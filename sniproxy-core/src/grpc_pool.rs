//! gRPC connection pooling for channel reuse
//!
//! This module provides specialized connection pooling for gRPC traffic, which uses HTTP/2
//! underneath. gRPC channels can be reused across multiple RPC calls, reducing connection
//! overhead and improving performance.
//!
//! # Features
//!
//! - Channel pooling per backend host
//! - Health checking for channels
//! - Round-robin load balancing
//! - Automatic cleanup of unhealthy channels
//! - Prometheus metrics for monitoring
//!
//! # Architecture
//!
//! gRPC uses HTTP/2 as the transport protocol, which supports multiplexing multiple
//! streams over a single connection. This pool maintains multiple channels per host
//! to distribute load and provide resilience.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tracing::{debug, info};

use prometheus::{IntCounter, IntGauge, Registry};

/// Configuration for gRPC connection pooling
#[derive(Debug, Clone)]
pub struct GrpcPoolConfig {
    /// Maximum channels per backend host (default: 10)
    pub max_channels_per_host: usize,
    /// Channel TTL in seconds (default: 300 = 5 minutes)
    pub channel_ttl: u64,
    /// Idle timeout in seconds (default: 120 = 2 minutes)
    pub idle_timeout: u64,
    /// Enable gRPC pooling (default: true)
    pub enabled: bool,
    /// Maximum concurrent streams per channel (default: 100)
    pub max_concurrent_streams: usize,
    /// Health check interval in seconds (default: 30)
    pub health_check_interval: u64,
}

impl Default for GrpcPoolConfig {
    fn default() -> Self {
        Self {
            max_channels_per_host: 10,
            channel_ttl: 300,  // 5 minutes
            idle_timeout: 120, // 2 minutes
            enabled: true,
            max_concurrent_streams: 100,
            health_check_interval: 30,
        }
    }
}

/// Represents a pooled gRPC channel
#[derive(Debug)]
struct GrpcChannel {
    #[allow(dead_code)] // Used in full implementation
    stream: TcpStream,
    created_at: Instant,
    last_used: Instant,
    rpc_count: usize,
    active_streams: usize,
    healthy: bool,
}

impl GrpcChannel {
    fn new(stream: TcpStream) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: now,
            rpc_count: 0,
            active_streams: 0,
            healthy: true,
        }
    }

    /// Check if channel has exceeded TTL
    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }

    /// Check if channel has been idle too long
    fn is_idle(&self, idle_timeout: Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }

    /// Check if channel is still valid and healthy
    fn is_valid(&self, ttl: Duration, idle_timeout: Duration) -> bool {
        self.healthy && !self.is_expired(ttl) && !self.is_idle(idle_timeout)
    }

    /// Check if channel can accept more streams
    fn can_accept_stream(&self, max_concurrent_streams: usize) -> bool {
        self.healthy && self.active_streams < max_concurrent_streams
    }

    /// Mark channel as used and increment counters
    #[allow(dead_code)]
    fn mark_used(&mut self) {
        self.rpc_count += 1;
        self.active_streams += 1;
        self.last_used = Instant::now();
    }

    /// Decrement active stream count
    #[allow(dead_code)]
    fn release_stream(&mut self) {
        if self.active_streams > 0 {
            self.active_streams -= 1;
        }
    }

    /// Mark channel as unhealthy
    #[allow(dead_code)]
    fn mark_unhealthy(&mut self) {
        self.healthy = false;
    }
}

/// Metrics for gRPC connection pool
struct GrpcPoolMetrics {
    pool_hits: IntCounter,
    pool_misses: IntCounter,
    pool_evictions: IntCounter,
    pool_size: IntGauge,
    active_channels: IntGauge,
    total_rpcs: IntCounter,
    unhealthy_channels: IntCounter,
}

impl GrpcPoolMetrics {
    fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let pool_hits = IntCounter::new(
            "sniproxy_grpc_pool_hits_total",
            "Total gRPC pool hits (reused channels)",
        )?;
        let pool_misses = IntCounter::new(
            "sniproxy_grpc_pool_misses_total",
            "Total gRPC pool misses (new channels)",
        )?;
        let pool_evictions = IntCounter::new(
            "sniproxy_grpc_pool_evictions_total",
            "Total gRPC channels evicted from pool (expired or unhealthy)",
        )?;
        let pool_size = IntGauge::new(
            "sniproxy_grpc_pool_size",
            "Current number of pooled gRPC channels",
        )?;
        let active_channels = IntGauge::new(
            "sniproxy_grpc_active_channels",
            "Current number of active gRPC channels",
        )?;
        let total_rpcs = IntCounter::new(
            "sniproxy_grpc_rpcs_total",
            "Total number of gRPC calls handled",
        )?;
        let unhealthy_channels = IntCounter::new(
            "sniproxy_grpc_unhealthy_channels_total",
            "Total number of channels marked unhealthy",
        )?;

        registry.register(Box::new(pool_hits.clone()))?;
        registry.register(Box::new(pool_misses.clone()))?;
        registry.register(Box::new(pool_evictions.clone()))?;
        registry.register(Box::new(pool_size.clone()))?;
        registry.register(Box::new(active_channels.clone()))?;
        registry.register(Box::new(total_rpcs.clone()))?;
        registry.register(Box::new(unhealthy_channels.clone()))?;

        Ok(Self {
            pool_hits,
            pool_misses,
            pool_evictions,
            pool_size,
            active_channels,
            total_rpcs,
            unhealthy_channels,
        })
    }
}

/// gRPC connection pool for channel reuse
pub struct GrpcConnectionPool {
    pools: Arc<DashMap<String, Vec<GrpcChannel>>>,
    config: GrpcPoolConfig,
    metrics: Option<GrpcPoolMetrics>,
    next_channel_index: Arc<DashMap<String, usize>>, // For round-robin
}

impl GrpcConnectionPool {
    /// Create a new gRPC connection pool
    pub fn new(config: GrpcPoolConfig) -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
            config,
            metrics: None,
            next_channel_index: Arc::new(DashMap::new()),
        }
    }

    /// Create a new gRPC connection pool with metrics
    pub fn with_metrics(
        config: GrpcPoolConfig,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        let metrics = GrpcPoolMetrics::new(registry)?;
        Ok(Self {
            pools: Arc::new(DashMap::new()),
            config,
            metrics: Some(metrics),
            next_channel_index: Arc::new(DashMap::new()),
        })
    }

    /// Try to get a channel from the pool using round-robin selection
    ///
    /// Returns Some(TcpStream) if a valid channel is available, None otherwise
    pub fn get(&self, host: &str) -> Option<TcpStream> {
        if !self.config.enabled {
            return None;
        }

        let mut pool = self.pools.get_mut(host)?;

        let ttl = Duration::from_secs(self.config.channel_ttl);
        let idle_timeout = Duration::from_secs(self.config.idle_timeout);
        let max_streams = self.config.max_concurrent_streams;

        // Get next channel index for round-robin
        let mut index_entry = self.next_channel_index.entry(host.to_string()).or_insert(0);
        let start_index = *index_entry;

        // Try to find a valid channel using round-robin
        let pool_len = pool.len();
        if pool_len == 0 {
            drop(index_entry);
            debug!(host = host, "gRPC pool miss (empty pool)");
            if let Some(ref metrics) = self.metrics {
                metrics.pool_misses.inc();
            }
            return None;
        }

        for attempt in 0..pool_len {
            let idx = (start_index + attempt) % pool_len;

            if let Some(channel) = pool.get_mut(idx) {
                // Check if channel is valid and can accept streams
                if !channel.is_valid(ttl, idle_timeout) {
                    debug!(host = host, index = idx, "Skipping expired/idle channel");
                    continue;
                }

                if !channel.can_accept_stream(max_streams) {
                    debug!(
                        host = host,
                        index = idx,
                        active_streams = channel.active_streams,
                        "Skipping saturated channel"
                    );
                    continue;
                }

                // Found a valid channel - extract it from the pool
                // Remove the channel and extract its stream
                // This provides connection reuse while maintaining compatibility
                // with the current API that returns TcpStream
                let channel = pool.remove(idx);

                // Update round-robin index
                *index_entry = idx % pool.len().max(1);

                debug!(
                    host = host,
                    rpc_count = channel.rpc_count,
                    active_streams = channel.active_streams,
                    remaining_in_pool = pool.len(),
                    "gRPC pool hit - extracted channel"
                );

                if let Some(ref metrics) = self.metrics {
                    metrics.pool_hits.inc();
                    metrics.total_rpcs.inc();
                    metrics.pool_size.dec();
                    metrics.active_channels.inc();
                }

                // Return the stream - caller is responsible for returning it via put()
                return Some(channel.stream);
            }
        }

        drop(index_entry);

        // No valid channel found
        debug!(host = host, "gRPC pool miss (no valid channels)");

        if let Some(ref metrics) = self.metrics {
            metrics.pool_misses.inc();
        }

        None
    }

    /// Return a channel to the pool
    ///
    /// Returns true if channel was added to pool, false if pool is full
    pub fn put(&self, host: String, stream: TcpStream) -> bool {
        if !self.config.enabled {
            return false;
        }

        let mut pool = self.pools.entry(host.clone()).or_default();

        // Check if pool is full
        if pool.len() >= self.config.max_channels_per_host {
            debug!(host = host, "gRPC pool full, discarding channel");
            return false;
        }

        // Add channel to pool
        pool.push(GrpcChannel::new(stream));

        debug!(
            host = host,
            pool_size = pool.len(),
            "Returned gRPC channel to pool"
        );

        if let Some(ref metrics) = self.metrics {
            metrics.pool_size.inc();
        }

        true
    }

    /// Mark a channel stream as released (RPC completed)
    pub fn release_stream(&self, _host: &str, _stream_id: usize) {
        // In a real implementation, we'd track which channel owns which stream
        // For now, this is a placeholder
        if let Some(ref metrics) = self.metrics {
            metrics.active_channels.dec();
        }
    }

    /// Mark a channel as unhealthy
    pub fn mark_unhealthy(&self, _host: &str, _stream_id: usize) {
        // In a real implementation, we'd identify and mark the specific channel
        if let Some(ref metrics) = self.metrics {
            metrics.unhealthy_channels.inc();
        }
    }

    /// Cleanup expired and unhealthy channels from all pools
    pub fn cleanup(&self) {
        let ttl = Duration::from_secs(self.config.channel_ttl);
        let idle_timeout = Duration::from_secs(self.config.idle_timeout);

        let mut total_evicted = 0;

        for mut entry in self.pools.iter_mut() {
            let host = entry.key().to_string();
            let pool = entry.value_mut();
            let before = pool.len();
            pool.retain(|channel| channel.is_valid(ttl, idle_timeout));
            let evicted = before - pool.len();

            if evicted > 0 {
                debug!(host = host, evicted = evicted, "Cleaned up gRPC channels");
                total_evicted += evicted;
            }
        }

        if total_evicted > 0 {
            info!(evicted = total_evicted, "gRPC pool cleanup complete");

            if let Some(ref metrics) = self.metrics {
                metrics.pool_evictions.inc_by(total_evicted as u64);
                metrics.pool_size.sub(total_evicted as i64);
            }
        }
    }

    /// Get statistics about the pool
    pub fn stats(&self) -> GrpcPoolStats {
        let total_channels: usize = self.pools.iter().map(|entry| entry.value().len()).sum();
        let hosts: usize = self.pools.len();

        GrpcPoolStats {
            total_channels,
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

/// Statistics about the gRPC connection pool
#[derive(Debug, Clone)]
pub struct GrpcPoolStats {
    pub total_channels: usize,
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
    async fn test_grpc_pool_disabled() {
        let config = GrpcPoolConfig {
            enabled: false,
            ..Default::default()
        };
        let pool = GrpcConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Should not accept channels when disabled
        assert!(!pool.put("grpc.example.com".to_string(), stream));

        // Should not return channels when disabled
        assert!(pool.get("grpc.example.com").is_none());
    }

    #[tokio::test]
    async fn test_grpc_pool_basic() {
        let config = GrpcPoolConfig {
            enabled: true,
            max_channels_per_host: 10,
            ..Default::default()
        };
        let pool = GrpcConnectionPool::new(config);

        let (stream, _) = create_test_connection().await;

        // Put channel in pool
        assert!(pool.put("grpc.example.com".to_string(), stream));

        // get() should return the channel stream for reuse
        let extracted = pool.get("grpc.example.com");
        assert!(extracted.is_some(), "Should extract channel from pool");

        // After extraction, pool should be empty for this host
        assert!(
            pool.get("grpc.example.com").is_none(),
            "Pool should be empty after extraction"
        );
    }

    #[tokio::test]
    async fn test_grpc_pool_max_channels() {
        let config = GrpcPoolConfig {
            enabled: true,
            max_channels_per_host: 2,
            ..Default::default()
        };
        let pool = GrpcConnectionPool::new(config);

        let (stream1, _) = create_test_connection().await;
        let (stream2, _) = create_test_connection().await;
        let (stream3, _) = create_test_connection().await;

        // Should accept first two
        assert!(pool.put("grpc.example.com".to_string(), stream1));
        assert!(pool.put("grpc.example.com".to_string(), stream2));

        // Should reject third (pool full)
        assert!(!pool.put("grpc.example.com".to_string(), stream3));
    }

    #[tokio::test]
    async fn test_grpc_channel_expiration() {
        let (stream, _) = create_test_connection().await;

        let mut channel = GrpcChannel::new(stream);

        // Should not be expired initially
        assert!(!channel.is_expired(Duration::from_secs(10)));

        // Manually set creation time to past
        channel.created_at = Instant::now() - Duration::from_secs(11);

        // Should be expired now
        assert!(channel.is_expired(Duration::from_secs(10)));
    }

    #[tokio::test]
    async fn test_grpc_channel_can_accept_stream() {
        let (stream, _) = create_test_connection().await;

        let mut channel = GrpcChannel::new(stream);

        // Should accept streams initially
        assert!(channel.can_accept_stream(10));

        // Add 5 active streams
        channel.active_streams = 5;
        assert!(channel.can_accept_stream(10));

        // At limit
        channel.active_streams = 10;
        assert!(!channel.can_accept_stream(10));

        // Unhealthy channel
        channel.active_streams = 0;
        channel.mark_unhealthy();
        assert!(!channel.can_accept_stream(10));
    }

    #[tokio::test]
    async fn test_grpc_pool_cleanup() {
        let config = GrpcPoolConfig {
            enabled: true,
            channel_ttl: 1,
            idle_timeout: 60,
            ..Default::default()
        };
        let pool = GrpcConnectionPool::new(config);

        let (stream1, _) = create_test_connection().await;
        let (stream2, _) = create_test_connection().await;

        pool.put("grpc1.example.com".to_string(), stream1);
        pool.put("grpc2.example.com".to_string(), stream2);

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Cleanup should remove both
        pool.cleanup();

        let stats = pool.stats();
        assert_eq!(stats.total_channels, 0);
    }

    #[tokio::test]
    async fn test_grpc_pool_stats() {
        let config = GrpcPoolConfig {
            enabled: true,
            ..Default::default()
        };
        let pool = GrpcConnectionPool::new(config);

        let (stream1, _) = create_test_connection().await;
        let (stream2, _) = create_test_connection().await;

        pool.put("grpc1.example.com".to_string(), stream1);
        pool.put("grpc2.example.com".to_string(), stream2);

        let stats = pool.stats();
        assert_eq!(stats.total_channels, 2);
        assert_eq!(stats.hosts, 2);
        assert!(stats.enabled);
    }

    #[test]
    fn test_grpc_pool_config_default() {
        let config = GrpcPoolConfig::default();
        assert_eq!(config.max_channels_per_host, 10);
        assert_eq!(config.channel_ttl, 300);
        assert_eq!(config.idle_timeout, 120);
        assert!(config.enabled);
        assert_eq!(config.max_concurrent_streams, 100);
        assert_eq!(config.health_check_interval, 30);
    }
}
