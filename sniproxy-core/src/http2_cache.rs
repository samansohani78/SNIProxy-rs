//! HTTP/2 Server Push Cache
//!
//! This module implements a cache for HTTP/2 server push promises, allowing
//! the proxy to track which resources have been pushed to clients and avoid
//! redundant pushes.
//!
//! # Features
//!
//! - LRU-based eviction policy for memory efficiency
//! - Configurable cache size and TTL
//! - Hit/miss rate tracking for monitoring
//! - Thread-safe concurrent access
//! - Automatic expiration of stale entries
//!
//! # Architecture
//!
//! HTTP/2 Server Push allows servers to proactively send resources before
//! they're requested. The cache tracks pushed resources to:
//! - Avoid duplicate pushes for the same resource
//! - Optimize bandwidth usage
//! - Achieve >95% cache hit rate for repeated resources

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Configuration for HTTP/2 push cache
#[derive(Debug, Clone)]
pub struct PushCacheConfig {
    /// Enable push cache (default: true)
    pub enabled: bool,
    /// Maximum number of entries in the cache (default: 1000)
    pub max_entries: usize,
    /// Time-to-live for cache entries in seconds (default: 300 = 5 minutes)
    pub ttl: u64,
    /// Enable automatic cleanup of expired entries (default: true)
    pub auto_cleanup: bool,
}

impl Default for PushCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 1000,
            ttl: 300, // 5 minutes
            auto_cleanup: true,
        }
    }
}

/// Entry in the HTTP/2 push cache
#[derive(Debug, Clone)]
struct PushCacheEntry {
    /// URL of the pushed resource
    #[allow(dead_code)]
    url: String,
    /// When this entry was created
    created_at: Instant,
    /// Number of times this entry was hit
    hit_count: usize,
    /// Size of the resource in bytes (if known)
    #[allow(dead_code)]
    size: Option<usize>,
}

impl PushCacheEntry {
    fn new(url: String, size: Option<usize>) -> Self {
        Self {
            url,
            created_at: Instant::now(),
            hit_count: 0,
            size,
        }
    }

    /// Check if this entry has expired
    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

/// HTTP/2 Server Push Cache
///
/// Tracks pushed resources to avoid redundant pushes and optimize bandwidth.
pub struct Http2PushCache {
    config: PushCacheConfig,
    cache: Arc<Mutex<LruCache<String, PushCacheEntry>>>,
    stats: Arc<Mutex<PushCacheStats>>,
}

impl Http2PushCache {
    /// Create a new HTTP/2 push cache
    ///
    /// # Arguments
    /// * `config` - Cache configuration
    ///
    /// # Returns
    /// * `Self` - New push cache instance
    pub fn new(config: PushCacheConfig) -> Self {
        let capacity =
            NonZeroUsize::new(config.max_entries).unwrap_or(NonZeroUsize::new(1000).unwrap());

        Self {
            config: config.clone(),
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            stats: Arc::new(Mutex::new(PushCacheStats::default())),
        }
    }

    /// Check if a resource should be pushed (not in cache or expired)
    ///
    /// # Arguments
    /// * `url` - URL of the resource to check
    ///
    /// # Returns
    /// * `bool` - True if resource should be pushed, false if already cached
    pub fn should_push(&self, url: &str) -> bool {
        if !self.config.enabled {
            return true; // Cache disabled, always push
        }

        let mut cache = self.cache.lock().unwrap();
        let ttl = Duration::from_secs(self.config.ttl);

        if let Some(entry) = cache.get_mut(url) {
            // Entry exists, check if expired
            if entry.is_expired(ttl) {
                // Expired, remove and indicate should push
                cache.pop(url);
                self.stats.lock().unwrap().misses += 1;
                true
            } else {
                // Valid entry, increment hit count
                entry.hit_count += 1;
                self.stats.lock().unwrap().hits += 1;
                false // Don't push, already cached
            }
        } else {
            // Not in cache, should push
            self.stats.lock().unwrap().misses += 1;
            true
        }
    }

    /// Record that a resource was pushed
    ///
    /// # Arguments
    /// * `url` - URL of the pushed resource
    /// * `size` - Optional size of the resource in bytes
    pub fn record_push(&self, url: String, size: Option<usize>) {
        if !self.config.enabled {
            return;
        }

        let entry = PushCacheEntry::new(url.clone(), size);
        let mut cache = self.cache.lock().unwrap();

        if cache.put(url.clone(), entry).is_some() {
            // Evicted an old entry
            self.stats.lock().unwrap().evictions += 1;
        }

        self.stats.lock().unwrap().pushes += 1;
    }

    /// Remove a resource from the cache
    ///
    /// # Arguments
    /// * `url` - URL of the resource to remove
    ///
    /// # Returns
    /// * `bool` - True if entry was removed, false if not found
    pub fn invalidate(&self, url: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        let mut cache = self.cache.lock().unwrap();
        cache.pop(url).is_some()
    }

    /// Clear all entries from the cache
    pub fn clear(&self) {
        if !self.config.enabled {
            return;
        }

        let mut cache = self.cache.lock().unwrap();
        cache.clear();

        let mut stats = self.stats.lock().unwrap();
        stats.evictions += cache.len();
    }

    /// Clean up expired entries
    ///
    /// # Returns
    /// * `usize` - Number of entries removed
    pub fn cleanup_expired(&self) -> usize {
        if !self.config.enabled || !self.config.auto_cleanup {
            return 0;
        }

        let ttl = Duration::from_secs(self.config.ttl);
        let mut cache = self.cache.lock().unwrap();
        let mut removed = 0;

        // Collect expired keys
        let expired_keys: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired(ttl))
            .map(|(key, _)| key.clone())
            .collect();

        // Remove expired entries
        for key in expired_keys {
            if cache.pop(&key).is_some() {
                removed += 1;
            }
        }

        if removed > 0 {
            let mut stats = self.stats.lock().unwrap();
            stats.evictions += removed;
        }

        removed
    }

    /// Get cache statistics
    ///
    /// # Returns
    /// * `PushCacheStats` - Current cache statistics
    pub fn stats(&self) -> PushCacheStats {
        self.stats.lock().unwrap().clone()
    }

    /// Get current cache size
    ///
    /// # Returns
    /// * `usize` - Number of entries in the cache
    pub fn len(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// Check if cache is empty
    ///
    /// # Returns
    /// * `bool` - True if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.lock().unwrap().is_empty()
    }

    /// Get cache hit rate
    ///
    /// # Returns
    /// * `f64` - Hit rate as a percentage (0.0 - 100.0)
    pub fn hit_rate(&self) -> f64 {
        let stats = self.stats.lock().unwrap();
        stats.hit_rate()
    }

    /// Get configuration
    pub fn config(&self) -> &PushCacheConfig {
        &self.config
    }
}

/// Statistics for HTTP/2 push cache
#[derive(Debug, Clone, Default)]
pub struct PushCacheStats {
    /// Total cache hits (resource already pushed)
    pub hits: usize,
    /// Total cache misses (resource not in cache)
    pub misses: usize,
    /// Total resources pushed
    pub pushes: usize,
    /// Total entries evicted (LRU or expired)
    pub evictions: usize,
}

impl PushCacheStats {
    /// Calculate cache hit rate as a percentage
    ///
    /// # Returns
    /// * `f64` - Hit rate (0.0 - 100.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        (self.hits as f64 / total as f64) * 100.0
    }

    /// Calculate total cache requests
    ///
    /// # Returns
    /// * `usize` - Total requests (hits + misses)
    pub fn total_requests(&self) -> usize {
        self.hits + self.misses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_push_cache_config_default() {
        let config = PushCacheConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_entries, 1000);
        assert_eq!(config.ttl, 300);
        assert!(config.auto_cleanup);
    }

    #[test]
    fn test_push_cache_basic() {
        let config = PushCacheConfig::default();
        let cache = Http2PushCache::new(config);

        // First check: should push (not in cache)
        assert!(cache.should_push("/style.css"));

        // Record the push
        cache.record_push("/style.css".to_string(), Some(1024));

        // Second check: should not push (in cache)
        assert!(!cache.should_push("/style.css"));
    }

    #[test]
    fn test_push_cache_disabled() {
        let config = PushCacheConfig {
            enabled: false,
            ..Default::default()
        };
        let cache = Http2PushCache::new(config);

        // Should always push when disabled
        assert!(cache.should_push("/style.css"));
        cache.record_push("/style.css".to_string(), Some(1024));
        assert!(cache.should_push("/style.css"));
    }

    #[test]
    fn test_push_cache_expiration() {
        let config = PushCacheConfig {
            ttl: 0, // Expire immediately
            ..Default::default()
        };
        let cache = Http2PushCache::new(config);

        cache.record_push("/style.css".to_string(), Some(1024));

        // Sleep briefly to ensure expiration
        thread::sleep(Duration::from_millis(10));

        // Should push again (expired)
        assert!(cache.should_push("/style.css"));
    }

    #[test]
    fn test_push_cache_lru_eviction() {
        let config = PushCacheConfig {
            max_entries: 2,
            ..Default::default()
        };
        let cache = Http2PushCache::new(config);

        // Fill cache
        cache.record_push("/file1.css".to_string(), Some(1024));
        cache.record_push("/file2.css".to_string(), Some(1024));

        // Access file1 to make it more recent
        assert!(!cache.should_push("/file1.css"));

        // Add file3, should evict file2 (least recently used)
        cache.record_push("/file3.css".to_string(), Some(1024));

        // file2 should be evicted
        assert!(cache.should_push("/file2.css"));
        // file1 should still be cached
        assert!(!cache.should_push("/file1.css"));
        // file3 should be cached
        assert!(!cache.should_push("/file3.css"));
    }

    #[test]
    fn test_push_cache_invalidate() {
        let config = PushCacheConfig::default();
        let cache = Http2PushCache::new(config);

        cache.record_push("/style.css".to_string(), Some(1024));
        assert!(!cache.should_push("/style.css"));

        // Invalidate the entry
        assert!(cache.invalidate("/style.css"));

        // Should push again (invalidated)
        assert!(cache.should_push("/style.css"));

        // Invalidating again should return false
        assert!(!cache.invalidate("/style.css"));
    }

    #[test]
    fn test_push_cache_clear() {
        let config = PushCacheConfig::default();
        let cache = Http2PushCache::new(config);

        cache.record_push("/file1.css".to_string(), Some(1024));
        cache.record_push("/file2.css".to_string(), Some(1024));
        cache.record_push("/file3.css".to_string(), Some(1024));

        assert_eq!(cache.len(), 3);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_push_cache_cleanup_expired() {
        let config = PushCacheConfig {
            ttl: 0, // Expire immediately
            auto_cleanup: true,
            ..Default::default()
        };
        let cache = Http2PushCache::new(config);

        cache.record_push("/file1.css".to_string(), Some(1024));
        cache.record_push("/file2.css".to_string(), Some(1024));

        // Sleep to ensure expiration
        thread::sleep(Duration::from_millis(10));

        let removed = cache.cleanup_expired();
        assert_eq!(removed, 2);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_push_cache_stats() {
        let config = PushCacheConfig::default();
        let cache = Http2PushCache::new(config);

        // Initial stats
        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.pushes, 0);

        // First access: miss
        assert!(cache.should_push("/style.css"));
        cache.record_push("/style.css".to_string(), Some(1024));

        // Second access: hit
        assert!(!cache.should_push("/style.css"));

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.pushes, 1);
        assert_eq!(stats.total_requests(), 2);
        assert_eq!(stats.hit_rate(), 50.0);
    }

    #[test]
    fn test_push_cache_hit_rate() {
        let config = PushCacheConfig::default();
        let cache = Http2PushCache::new(config);

        // First check: miss
        assert!(cache.should_push("/style.css"));
        // Record the resource
        cache.record_push("/style.css".to_string(), Some(1024));

        // Access it 9 times (all hits)
        for _ in 0..9 {
            assert!(!cache.should_push("/style.css"));
        }

        // Hit rate should be 90% (9 hits, 1 miss)
        assert!((cache.hit_rate() - 90.0).abs() < 0.1);
    }

    #[test]
    fn test_push_cache_multiple_resources() {
        let config = PushCacheConfig::default();
        let cache = Http2PushCache::new(config);

        // Push multiple resources
        let resources = vec!["/style.css", "/script.js", "/image.png", "/font.woff2"];

        for resource in &resources {
            assert!(cache.should_push(resource));
            cache.record_push(resource.to_string(), Some(1024));
        }

        // All should be cached
        for resource in &resources {
            assert!(!cache.should_push(resource));
        }

        assert_eq!(cache.len(), 4);
    }
}
