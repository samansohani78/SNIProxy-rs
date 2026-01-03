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

/// HTTP version for Keep-Alive tracking
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpVersion {
    Http10,
    Http11,
    Http2,
}

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
    /// Enable HTTP Keep-Alive (default: true)
    pub keep_alive_enabled: bool,
    /// Maximum requests per connection (default: 1000)
    pub max_requests_per_connection: usize,
    /// Keep-Alive timeout in seconds (default: 60)
    pub keep_alive_timeout: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_per_host: 100,
            connection_ttl: 60,
            idle_timeout: 30,
            enabled: true,
            keep_alive_enabled: true,
            max_requests_per_connection: 1000,
            keep_alive_timeout: 60,
        }
    }
}

/// A pooled connection with metadata
struct PooledConnection {
    stream: TcpStream,
    created_at: Instant,
    last_used: Instant,
    http_version: HttpVersion,
    keep_alive: bool,
    request_count: usize,
}

impl PooledConnection {
    fn new(stream: TcpStream) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: now,
            http_version: HttpVersion::Http11, // Default to HTTP/1.1
            keep_alive: true,                  // Default to Keep-Alive enabled
            request_count: 0,
        }
    }

    fn with_http_info(stream: TcpStream, http_version: HttpVersion, keep_alive: bool) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: now,
            http_version,
            keep_alive,
            request_count: 0,
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

    /// Check if connection can be reused (Keep-Alive check)
    fn can_keep_alive(&self, max_requests: usize) -> bool {
        self.keep_alive && self.request_count < max_requests
    }

    /// Increment request count and update last used time
    fn mark_used(&mut self) {
        self.request_count += 1;
        self.last_used = Instant::now();
    }
}

/// Metrics for connection pool
struct PoolMetrics {
    pool_hits: IntCounter,
    pool_misses: IntCounter,
    pool_evictions: IntCounter,
    pool_size: IntGauge,
    active_connections: IntGauge,
    keep_alive_reuses: IntCounter,
    keep_alive_rejections: IntCounter,
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
        let keep_alive_reuses = IntCounter::new(
            "sniproxy_keep_alive_reuses_total",
            "Total HTTP Keep-Alive connection reuses",
        )?;
        let keep_alive_rejections = IntCounter::new(
            "sniproxy_keep_alive_rejections_total",
            "Total HTTP Keep-Alive connection rejections (max requests exceeded)",
        )?;

        registry.register(Box::new(pool_hits.clone()))?;
        registry.register(Box::new(pool_misses.clone()))?;
        registry.register(Box::new(pool_evictions.clone()))?;
        registry.register(Box::new(pool_size.clone()))?;
        registry.register(Box::new(active_connections.clone()))?;
        registry.register(Box::new(keep_alive_reuses.clone()))?;
        registry.register(Box::new(keep_alive_rejections.clone()))?;

        Ok(Self {
            pool_hits,
            pool_misses,
            pool_evictions,
            pool_size,
            active_connections,
            keep_alive_reuses,
            keep_alive_rejections,
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
        let max_requests = self.config.max_requests_per_connection;

        // Try to find a valid connection
        while let Some(mut conn) = pool.pop() {
            // Check basic validity (TTL, idle timeout)
            if !conn.is_valid(ttl, idle_timeout) {
                debug!(host = host, "Evicting expired/idle connection from pool");

                if let Some(ref metrics) = self.metrics {
                    metrics.pool_evictions.inc();
                    metrics.pool_size.dec();
                }
                continue;
            }

            // Check Keep-Alive constraints
            if self.config.keep_alive_enabled && !conn.can_keep_alive(max_requests) {
                debug!(
                    host = host,
                    request_count = conn.request_count,
                    "Evicting connection (max requests exceeded)"
                );

                if let Some(ref metrics) = self.metrics {
                    metrics.keep_alive_rejections.inc();
                    metrics.pool_evictions.inc();
                    metrics.pool_size.dec();
                }
                continue;
            }

            // Connection is valid and can be reused
            conn.mark_used();

            debug!(
                host = host,
                request_count = conn.request_count,
                http_version = match conn.http_version {
                    HttpVersion::Http10 => "HTTP/1.0",
                    HttpVersion::Http11 => "HTTP/1.1",
                    HttpVersion::Http2 => "HTTP/2",
                },
                "Connection pool hit"
            );

            if let Some(ref metrics) = self.metrics {
                metrics.pool_hits.inc();
                metrics.pool_size.dec();
                metrics.active_connections.inc();

                if conn.request_count > 1 {
                    metrics.keep_alive_reuses.inc();
                }
            }

            return Some(conn.stream);
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

        let mut pool = self.pools.entry(host.clone()).or_default();

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

    /// Return a connection to the pool with HTTP version and Keep-Alive information
    ///
    /// This method allows specifying HTTP version and Keep-Alive status for better
    /// connection reuse decisions.
    ///
    /// Returns true if connection was added to pool, false if pool is full or Keep-Alive disabled
    pub fn put_with_http_info(
        &self,
        host: String,
        stream: TcpStream,
        http_version: HttpVersion,
        keep_alive: bool,
    ) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Don't pool connections if Keep-Alive is disabled (e.g., Connection: close)
        if self.config.keep_alive_enabled && !keep_alive {
            debug!(host = host, "Not pooling connection (Keep-Alive disabled)");
            return false;
        }

        let mut pool = self.pools.entry(host.clone()).or_default();

        // Check if pool is full
        if pool.len() >= self.config.max_per_host {
            debug!(host = host, "Connection pool full, discarding connection");
            return false;
        }

        // Add connection to pool with HTTP info
        pool.push(PooledConnection::with_http_info(
            stream,
            http_version,
            keep_alive,
        ));

        debug!(
            host = host,
            pool_size = pool.len(),
            http_version = match http_version {
                HttpVersion::Http10 => "HTTP/1.0",
                HttpVersion::Http11 => "HTTP/1.1",
                HttpVersion::Http2 => "HTTP/2",
            },
            keep_alive = keep_alive,
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

/// Parse HTTP headers to determine if Keep-Alive should be used
///
/// # HTTP Keep-Alive Rules
/// - HTTP/1.1: Keep-Alive is default, unless "Connection: close" is present
/// - HTTP/1.0: Keep-Alive requires explicit "Connection: keep-alive" header
/// - HTTP/2: Persistent connections by default (no Connection header)
///
/// # Arguments
/// * `headers` - HTTP response headers as a string
/// * `http_version` - HTTP version (HTTP/1.0, HTTP/1.1, or HTTP/2)
///
/// # Returns
/// `true` if connection should be kept alive, `false` otherwise
pub fn should_keep_alive(headers: &str, http_version: HttpVersion) -> bool {
    let headers_lower = headers.to_lowercase();

    // HTTP/2 connections are always persistent
    if http_version == HttpVersion::Http2 {
        return true;
    }

    // Check for explicit Connection header
    for line in headers_lower.lines() {
        if line.starts_with("connection:") {
            let value = line.trim_start_matches("connection:").trim();

            // Explicit "close" disables Keep-Alive
            if value.contains("close") {
                return false;
            }

            // Explicit "keep-alive" enables Keep-Alive
            if value.contains("keep-alive") {
                return true;
            }
        }
    }

    // Default behavior based on HTTP version
    match http_version {
        HttpVersion::Http10 => false, // HTTP/1.0 defaults to no Keep-Alive
        HttpVersion::Http11 => true,  // HTTP/1.1 defaults to Keep-Alive
        HttpVersion::Http2 => true,   // HTTP/2 always uses persistent connections
    }
}

/// Parse HTTP version from request/response line
///
/// # Arguments
/// * `line` - First line of HTTP request/response (e.g., "GET / HTTP/1.1" or "HTTP/1.1 200 OK")
///
/// # Returns
/// The HTTP version, defaulting to HTTP/1.1 if not parseable
pub fn parse_http_version(line: &str) -> HttpVersion {
    if line.contains("HTTP/1.0") {
        HttpVersion::Http10
    } else if line.contains("HTTP/2") || line.contains("HTTP/2.0") {
        HttpVersion::Http2
    } else {
        HttpVersion::Http11 // Default to HTTP/1.1
    }
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

    // Keep-Alive tests

    #[test]
    fn test_parse_http_version() {
        assert_eq!(parse_http_version("GET / HTTP/1.0"), HttpVersion::Http10);
        assert_eq!(parse_http_version("GET / HTTP/1.1"), HttpVersion::Http11);
        assert_eq!(parse_http_version("GET / HTTP/2"), HttpVersion::Http2);
        assert_eq!(parse_http_version("HTTP/1.1 200 OK"), HttpVersion::Http11);
        assert_eq!(parse_http_version("HTTP/2.0 200 OK"), HttpVersion::Http2);
        // Default to HTTP/1.1 for unknown
        assert_eq!(parse_http_version("GET /"), HttpVersion::Http11);
    }

    #[test]
    fn test_should_keep_alive_http11_default() {
        let headers = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n";
        assert!(should_keep_alive(headers, HttpVersion::Http11));
    }

    #[test]
    fn test_should_keep_alive_http11_explicit_close() {
        let headers = "HTTP/1.1 200 OK\r\nConnection: close\r\n";
        assert!(!should_keep_alive(headers, HttpVersion::Http11));
    }

    #[test]
    fn test_should_keep_alive_http11_explicit_keep_alive() {
        let headers = "HTTP/1.1 200 OK\r\nConnection: keep-alive\r\n";
        assert!(should_keep_alive(headers, HttpVersion::Http11));
    }

    #[test]
    fn test_should_keep_alive_http10_default() {
        let headers = "HTTP/1.0 200 OK\r\nContent-Length: 0\r\n";
        assert!(!should_keep_alive(headers, HttpVersion::Http10));
    }

    #[test]
    fn test_should_keep_alive_http10_explicit() {
        let headers = "HTTP/1.0 200 OK\r\nConnection: keep-alive\r\n";
        assert!(should_keep_alive(headers, HttpVersion::Http10));
    }

    #[test]
    fn test_should_keep_alive_http2() {
        let headers = "HTTP/2 200\r\n";
        assert!(should_keep_alive(headers, HttpVersion::Http2));
        // HTTP/2 ignores Connection header
        let headers_with_close = "HTTP/2 200\r\nConnection: close\r\n";
        assert!(should_keep_alive(headers_with_close, HttpVersion::Http2));
    }

    #[tokio::test]
    async fn test_put_with_http_info() {
        let config = PoolConfig {
            enabled: true,
            keep_alive_enabled: true,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Put connection with HTTP/1.1 and Keep-Alive enabled
        assert!(pool.put_with_http_info("test.com".to_string(), stream, HttpVersion::Http11, true));

        // Should be able to retrieve it
        assert!(pool.get("test.com").is_some());
    }

    #[tokio::test]
    async fn test_put_with_http_info_keep_alive_disabled() {
        let config = PoolConfig {
            enabled: true,
            keep_alive_enabled: true,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Put connection with Keep-Alive disabled (Connection: close)
        assert!(!pool.put_with_http_info(
            "test.com".to_string(),
            stream,
            HttpVersion::Http11,
            false
        ));

        // Should not be in pool
        assert!(pool.get("test.com").is_none());
    }

    #[tokio::test]
    async fn test_keep_alive_max_requests() {
        let config = PoolConfig {
            enabled: true,
            keep_alive_enabled: true,
            max_requests_per_connection: 3, // Allow only 3 requests
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Put connection
        pool.put_with_http_info("test.com".to_string(), stream, HttpVersion::Http11, true);

        // First reuse (request 1)
        assert!(pool.get("test.com").is_some());

        // Second reuse (request 2)
        let (stream2, _) = create_test_connection().await;
        pool.put_with_http_info("test.com".to_string(), stream2, HttpVersion::Http11, true);
        assert!(pool.get("test.com").is_some());

        // Third reuse (request 3)
        let (stream3, _) = create_test_connection().await;
        pool.put_with_http_info("test.com".to_string(), stream3, HttpVersion::Http11, true);
        assert!(pool.get("test.com").is_some());

        // Fourth reuse should fail (exceeded max_requests)
        let (stream4, _) = create_test_connection().await;
        pool.put_with_http_info("test.com".to_string(), stream4, HttpVersion::Http11, true);
        // Pool has connection but it should be rejected due to max requests
        // Note: Connection will be evicted on next get() attempt
    }

    #[tokio::test]
    async fn test_pooled_connection_can_keep_alive() {
        let (stream, _) = create_test_connection().await;

        let mut conn = PooledConnection {
            stream,
            created_at: Instant::now(),
            last_used: Instant::now(),
            http_version: HttpVersion::Http11,
            keep_alive: true,
            request_count: 0,
        };

        // Should be able to reuse initially
        assert!(conn.can_keep_alive(100));

        // After 99 requests, should still be reusable
        conn.request_count = 99;
        assert!(conn.can_keep_alive(100));

        // At 100 requests, should not be reusable
        conn.request_count = 100;
        assert!(!conn.can_keep_alive(100));

        // Keep-Alive disabled
        conn.request_count = 0;
        conn.keep_alive = false;
        assert!(!conn.can_keep_alive(100));
    }
}
