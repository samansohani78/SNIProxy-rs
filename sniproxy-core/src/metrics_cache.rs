//! Metrics label caching to reduce allocations
//!
//! Pre-allocates and caches metric label strings to avoid
//! repeated format!() and to_string() calls on hot paths.

use dashmap::DashMap;
use std::sync::Arc;

/// Cache for metric labels to reduce string allocations
pub struct MetricLabelCache {
    // Cache format: (host, protocol) -> "host-protocol"
    cache: DashMap<(String, String), Arc<str>>,
}

impl MetricLabelCache {
    /// Create a new label cache
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
        }
    }

    /// Get or create a cached label for host+protocol
    ///
    /// Returns Arc<str> which can be cheaply cloned
    pub fn get_or_insert(&self, host: &str, protocol: &str) -> Arc<str> {
        self.cache
            .entry((host.to_string(), protocol.to_string()))
            .or_insert_with(|| Arc::from(format!("{}-{}", host, protocol)))
            .clone()
    }

    /// Get or create a cached label for a single string
    pub fn get_or_insert_single(&self, label: &str) -> Arc<str> {
        self.cache
            .entry((label.to_string(), String::new()))
            .or_insert_with(|| Arc::from(label))
            .clone()
    }

    /// Clear the cache (for testing/maintenance)
    pub fn clear(&self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for MetricLabelCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_cache_basic() {
        let cache = MetricLabelCache::new();
        let label1 = cache.get_or_insert("example.com", "http1.1");
        let label2 = cache.get_or_insert("example.com", "http1.1");

        // Should return same Arc (pointer equality)
        assert!(Arc::ptr_eq(&label1, &label2));
        assert_eq!(label1.as_ref(), "example.com-http1.1");
    }

    #[test]
    fn test_label_cache_different_entries() {
        let cache = MetricLabelCache::new();
        let label1 = cache.get_or_insert("example.com", "http1.1");
        let label2 = cache.get_or_insert("example.com", "http2");

        assert!(!Arc::ptr_eq(&label1, &label2));
        assert_eq!(label1.as_ref(), "example.com-http1.1");
        assert_eq!(label2.as_ref(), "example.com-http2");
    }

    #[test]
    fn test_label_cache_single() {
        let cache = MetricLabelCache::new();
        let label1 = cache.get_or_insert_single("test-label");
        let label2 = cache.get_or_insert_single("test-label");

        assert!(Arc::ptr_eq(&label1, &label2));
        assert_eq!(label1.as_ref(), "test-label");
    }

    #[test]
    fn test_cache_len() {
        let cache = MetricLabelCache::new();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());

        cache.get_or_insert("example.com", "http1.1");
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());

        cache.get_or_insert("example.com", "http2");
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_cache_clear() {
        let cache = MetricLabelCache::new();
        cache.get_or_insert("example.com", "http1.1");
        cache.get_or_insert("example.com", "http2");
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }
}
