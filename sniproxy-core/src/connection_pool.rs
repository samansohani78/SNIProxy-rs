//! Connection pooling for backend connections
//!
//! This module provides connection pooling functionality to reuse backend connections,
//! reducing file descriptor usage and improving performance.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tracing::{debug, info};

use prometheus::{IntCounter, IntGauge, Registry};

/// Configuration for connection pooling
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum connections per backend host (default: 100)
    pub max_per_host: usize,
    /// Connection TTL in seconds (default: 60)
    pub connection_ttl: u64,
    /// Idle timeout in seconds (default: 30)
    pub idle_timeout: u64,
    /// Enable connection pooling (default: true)
    pub enabled: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_per_host: 100,
            connection_ttl: 60,
            idle_timeout: 30,
            enabled: true,
        }
    }
}

/// A pooled connection with metadata
struct PooledConnection {
    stream: TcpStream,
    created_at: Instant,
    last_used: Instant,
}

impl PooledConnection {
    fn new(stream: TcpStream) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: now,
        }
    }

    /// Check if connection has exceeded TTL
    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }

    /// Check if connection has been idle too long
    fn is_idle(&self, idle_timeout: Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }

    /// Check if connection is still valid
    fn is_valid(&self, ttl: Duration, idle_timeout: Duration) -> bool {
        !self.is_expired(ttl) && !self.is_idle(idle_timeout)
    }
}

/// Metrics for connection pool
struct PoolMetrics {
    pool_hits: IntCounter,
    pool_misses: IntCounter,
    pool_evictions: IntCounter,
    pool_size: IntGauge,
    active_connections: IntGauge,
}

impl PoolMetrics {
    fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let pool_hits = IntCounter::new(
            "sniproxy_pool_hits_total",
            "Total connection pool hits (reused connections)",
        )?;
        let pool_misses = IntCounter::new(
            "sniproxy_pool_misses_total",
            "Total connection pool misses (new connections)",
        )?;
        let pool_evictions = IntCounter::new(
            "sniproxy_pool_evictions_total",
            "Total connections evicted from pool (expired or idle)",
        )?;
        let pool_size =
            IntGauge::new("sniproxy_pool_size", "Current number of pooled connections")?;
        let active_connections = IntGauge::new(
            "sniproxy_pool_active_connections",
            "Current number of active connections from pool",
        )?;

        registry.register(Box::new(pool_hits.clone()))?;
        registry.register(Box::new(pool_misses.clone()))?;
        registry.register(Box::new(pool_evictions.clone()))?;
        registry.register(Box::new(pool_size.clone()))?;
        registry.register(Box::new(active_connections.clone()))?;

        Ok(Self {
            pool_hits,
            pool_misses,
            pool_evictions,
            pool_size,
            active_connections,
        })
    }
}

/// Connection pool for backend connections
pub struct ConnectionPool {
    pools: Arc<DashMap<String, Vec<PooledConnection>>>,
    config: PoolConfig,
    metrics: Option<PoolMetrics>,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
            config,
            metrics: None,
        }
    }

    /// Create a new connection pool with metrics
    pub fn with_metrics(
        config: PoolConfig,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        let metrics = PoolMetrics::new(registry)?;
        Ok(Self {
            pools: Arc::new(DashMap::new()),
            config,
            metrics: Some(metrics),
        })
    }

    /// Try to get a connection from the pool
    ///
    /// Returns Some(TcpStream) if a valid connection is available, None otherwise
    pub fn get(&self, host: &str) -> Option<TcpStream> {
        if !self.config.enabled {
            return None;
        }

        let mut pool = self.pools.get_mut(host)?;

        let ttl = Duration::from_secs(self.config.connection_ttl);
        let idle_timeout = Duration::from_secs(self.config.idle_timeout);

        // Try to find a valid connection
        while let Some(mut conn) = pool.pop() {
            if conn.is_valid(ttl, idle_timeout) {
                // Update last used time
                conn.last_used = Instant::now();

                debug!(host = host, "Connection pool hit");

                if let Some(ref metrics) = self.metrics {
                    metrics.pool_hits.inc();
                    metrics.pool_size.dec();
                    metrics.active_connections.inc();
                }

                return Some(conn.stream);
            } else {
                debug!(host = host, "Evicting expired/idle connection from pool");

                if let Some(ref metrics) = self.metrics {
                    metrics.pool_evictions.inc();
                    metrics.pool_size.dec();
                }
            }
        }

        // No valid connection found
        debug!(host = host, "Connection pool miss");

        if let Some(ref metrics) = self.metrics {
            metrics.pool_misses.inc();
        }

        None
    }

    /// Return a connection to the pool
    ///
    /// Returns true if connection was added to pool, false if pool is full
    pub fn put(&self, host: String, stream: TcpStream) -> bool {
        if !self.config.enabled {
            return false;
        }

        let mut pool = self.pools.entry(host.clone()).or_insert(Vec::new());

        // Check if pool is full
        if pool.len() >= self.config.max_per_host {
            debug!(host = host, "Connection pool full, discarding connection");
            return false;
        }

        // Add connection to pool
        pool.push(PooledConnection::new(stream));

        debug!(
            host = host,
            pool_size = pool.len(),
            "Returned connection to pool"
        );

        if let Some(ref metrics) = self.metrics {
            metrics.pool_size.inc();
            metrics.active_connections.dec();
        }

        true
    }

    /// Mark a connection as no longer active (failed or closed)
    pub fn mark_inactive(&self) {
        if let Some(ref metrics) = self.metrics {
            metrics.active_connections.dec();
        }
    }

    /// Cleanup expired connections from all pools
    pub fn cleanup(&self) {
        let ttl = Duration::from_secs(self.config.connection_ttl);
        let idle_timeout = Duration::from_secs(self.config.idle_timeout);

        let mut total_evicted = 0;

        for mut entry in self.pools.iter_mut() {
            let host = entry.key().to_string(); // Clone the key to avoid borrow conflict
            let pool = entry.value_mut();
            let before = pool.len();
            pool.retain(|conn| conn.is_valid(ttl, idle_timeout));
            let evicted = before - pool.len();

            if evicted > 0 {
                debug!(
                    host = host,
                    evicted = evicted,
                    "Cleaned up expired connections"
                );
                total_evicted += evicted;
            }
        }

        if total_evicted > 0 {
            info!(evicted = total_evicted, "Connection pool cleanup complete");

            if let Some(ref metrics) = self.metrics {
                metrics.pool_evictions.inc_by(total_evicted as u64);
                metrics.pool_size.sub(total_evicted as i64);
            }
        }
    }

    /// Get statistics about the pool
    pub fn stats(&self) -> PoolStats {
        let total_connections: usize = self.pools.iter().map(|entry| entry.value().len()).sum();
        let hosts: usize = self.pools.len();

        PoolStats {
            total_connections,
            hosts,
            enabled: self.config.enabled,
        }
    }

    /// Start background cleanup task
    ///
    /// Returns a JoinHandle that will run cleanup every interval
    pub fn start_cleanup_task(self: Arc<Self>, interval: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                self.cleanup();
            }
        })
    }
}

/// Statistics about the connection pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_connections: usize,
    pub hosts: usize,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn create_test_connection() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_fut = TcpStream::connect(addr);
        let server_fut = listener.accept();

        let (client_result, server_result) = tokio::join!(client_fut, server_fut);

        (client_result.unwrap(), server_result.unwrap().0)
    }

    #[tokio::test]
    async fn test_pool_disabled() {
        let config = PoolConfig {
            enabled: false,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Should not accept connections when disabled
        assert!(!pool.put("test.com".to_string(), stream));

        // Should not return connections when disabled
        assert!(pool.get("test.com").is_none());
    }

    #[tokio::test]
    async fn test_pool_basic() {
        let config = PoolConfig {
            enabled: true,
            max_per_host: 10,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Put connection in pool
        assert!(pool.put("test.com".to_string(), stream));

        // Get connection from pool
        let retrieved = pool.get("test.com");
        assert!(retrieved.is_some());

        // Pool should be empty now
        assert!(pool.get("test.com").is_none());
    }

    #[tokio::test]
    async fn test_pool_max_per_host() {
        let config = PoolConfig {
            enabled: true,
            max_per_host: 2,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream1, _) = create_test_connection().await;
        let (stream2, _) = create_test_connection().await;
        let (stream3, _) = create_test_connection().await;

        // Should accept first two
        assert!(pool.put("test.com".to_string(), stream1));
        assert!(pool.put("test.com".to_string(), stream2));

        // Should reject third (pool full)
        assert!(!pool.put("test.com".to_string(), stream3));
    }

    #[tokio::test]
    async fn test_pool_expiration() {
        let config = PoolConfig {
            enabled: true,
            connection_ttl: 1, // 1 second TTL
            idle_timeout: 60,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Put connection in pool
        assert!(pool.put("test.com".to_string(), stream));

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should not get expired connection
        assert!(pool.get("test.com").is_none());
    }

    #[tokio::test]
    async fn test_pool_cleanup() {
        let config = PoolConfig {
            enabled: true,
            connection_ttl: 1,
            idle_timeout: 60,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream1, _) = create_test_connection().await;
        let (stream2, _) = create_test_connection().await;

        pool.put("test1.com".to_string(), stream1);
        pool.put("test2.com".to_string(), stream2);

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Cleanup should remove both
        pool.cleanup();

        let stats = pool.stats();
        assert_eq!(stats.total_connections, 0);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let config = PoolConfig {
            enabled: true,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream1, _) = create_test_connection().await;
        let (stream2, _) = create_test_connection().await;

        pool.put("host1.com".to_string(), stream1);
        pool.put("host2.com".to_string(), stream2);

        let stats = pool.stats();
        assert_eq!(stats.total_connections, 2);
        assert_eq!(stats.hosts, 2);
        assert!(stats.enabled);
    }
}
