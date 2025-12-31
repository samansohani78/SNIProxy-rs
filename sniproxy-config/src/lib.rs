use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// SNIProxy configuration loaded from YAML.
///
/// This structure defines all configuration options for the proxy server including
/// listen addresses, timeout settings, metrics configuration, and domain allowlist.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// List of addresses to listen on (e.g., "0.0.0.0:443", "[::]:443")
    pub listen_addrs: Vec<String>,
    /// Timeout configuration for various operations
    pub timeouts: Timeouts,
    /// Prometheus metrics configuration
    pub metrics: Metrics,
    /// Optional list of allowed domains (supports wildcards like "*.example.com")
    pub allowlist: Option<Vec<String>>,
    /// Maximum number of concurrent connections (default: 10000 if not specified)
    #[serde(default)]
    pub max_connections: Option<usize>,
    /// Graceful shutdown timeout in seconds (default: 30 if not specified)
    #[serde(default)]
    pub shutdown_timeout: Option<u64>,
    /// Connection pooling configuration (optional)
    #[serde(default)]
    pub connection_pool: Option<ConnectionPool>,
}

/// Connection pooling configuration.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectionPool {
    /// Enable connection pooling (default: true)
    #[serde(default = "default_pool_enabled")]
    pub enabled: bool,
    /// Maximum connections per backend host (default: 100)
    #[serde(default = "default_max_per_host")]
    pub max_per_host: usize,
    /// Connection TTL in seconds (default: 60)
    #[serde(default = "default_connection_ttl")]
    pub connection_ttl: u64,
    /// Idle timeout in seconds (default: 30)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,
    /// Cleanup interval in seconds (default: 10)
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval: u64,
}

fn default_pool_enabled() -> bool {
    true
}

fn default_max_per_host() -> usize {
    100
}

fn default_connection_ttl() -> u64 {
    60
}

fn default_idle_timeout() -> u64 {
    30
}

fn default_cleanup_interval() -> u64 {
    10
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self {
            enabled: default_pool_enabled(),
            max_per_host: default_max_per_host(),
            connection_ttl: default_connection_ttl(),
            idle_timeout: default_idle_timeout(),
            cleanup_interval: default_cleanup_interval(),
        }
    }
}

/// Timeout settings for proxy operations (all values in seconds).
#[derive(Debug, Serialize, Deserialize)]
pub struct Timeouts {
    /// Maximum time to establish backend connection (default: 10s)
    pub connect: u64,
    /// Maximum time to receive TLS ClientHello or HTTP headers (default: 10s)
    pub client_hello: u64,
    /// Maximum idle time for established connections (default: 300s)
    pub idle: u64,
}

/// Prometheus metrics server configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct Metrics {
    /// Whether to enable metrics collection
    pub enabled: bool,
    /// Address to bind metrics HTTP server (e.g., "127.0.0.1:9000")
    pub address: String,
}

impl Config {
    /// Loads configuration from a YAML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the YAML configuration file
    ///
    /// # Returns
    ///
    /// Returns the parsed configuration or an error if the file cannot be read or parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use sniproxy_config::Config;
    /// use std::path::Path;
    ///
    /// let config = Config::from_file(Path::new("config.yaml")).unwrap();
    /// ```
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config = serde_yml::from_str(&contents)?;
        Ok(config)
    }

    /// Parses configuration from a YAML string.
    ///
    /// This is primarily used for testing and programmatic configuration.
    ///
    /// # Arguments
    ///
    /// * `contents` - YAML configuration as a string
    ///
    /// # Examples
    ///
    /// ```
    /// use sniproxy_config::Config;
    ///
    /// let yaml = r#"
    /// listen_addrs:
    ///   - "0.0.0.0:443"
    /// timeouts:
    ///   connect: 10
    ///   client_hello: 10
    ///   idle: 300
    /// metrics:
    ///   enabled: true
    ///   address: "127.0.0.1:9000"
    /// "#;
    ///
    /// let config = Config::parse(yaml).unwrap();
    /// assert_eq!(config.listen_addrs[0], "0.0.0.0:443");
    /// ```
    pub fn parse(contents: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config = serde_yml::from_str(contents)?;
        Ok(config)
    }
}

/// Checks if a hostname matches an allowlist pattern.
///
/// Supports wildcard patterns for flexible domain matching:
/// - Exact match: `"example.com"` matches only `"example.com"`
/// - Subdomain wildcard: `"*.example.com"` matches `"api.example.com"`, `"www.example.com"`, and `"example.com"`
/// - Suffix wildcard: `"*api.com"` matches `"api.com"`, `"testapi.com"`, etc.
///
/// # Arguments
///
/// * `hostname` - The hostname to check
/// * `pattern` - The allowlist pattern (supports `*` wildcard)
///
/// # Examples
///
/// ```
/// use sniproxy_config::matches_allowlist_pattern;
///
/// // Exact match
/// assert!(matches_allowlist_pattern("example.com", "example.com"));
///
/// // Subdomain wildcard
/// assert!(matches_allowlist_pattern("api.example.com", "*.example.com"));
/// assert!(matches_allowlist_pattern("example.com", "*.example.com"));
///
/// // Suffix wildcard
/// assert!(matches_allowlist_pattern("myapi.com", "*api.com"));
/// ```
pub fn matches_allowlist_pattern(hostname: &str, pattern: &str) -> bool {
    if pattern == hostname {
        return true;
    }

    // Handle wildcard patterns like "*.example.com"
    if let Some(domain) = pattern.strip_prefix("*.") {
        // Remove "*."
        // hostname should end with .domain (e.g., "sub.example.com" matches "*.example.com")
        hostname.ends_with(&format!(".{}", domain)) || hostname == domain
    } else if let Some(suffix) = pattern.strip_prefix("*") {
        hostname.ends_with(suffix)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config_parsing() {
        let yaml = r#"
listen_addrs:
  - "0.0.0.0:80"
  - "0.0.0.0:443"
timeouts:
  connect: 10
  client_hello: 10
  idle: 300
metrics:
  enabled: true
  address: "127.0.0.1:9000"
allowlist:
  - "example.com"
  - "*.example.com"
"#;
        let config = Config::parse(yaml).unwrap();
        assert_eq!(config.listen_addrs.len(), 2);
        assert_eq!(config.listen_addrs[0], "0.0.0.0:80");
        assert_eq!(config.timeouts.connect, 10);
        assert_eq!(config.timeouts.client_hello, 10);
        assert_eq!(config.timeouts.idle, 300);
        assert!(config.metrics.enabled);
        assert_eq!(config.metrics.address, "127.0.0.1:9000");
        assert!(config.allowlist.is_some());
        let allowlist = config.allowlist.unwrap();
        assert_eq!(allowlist.len(), 2);
        assert_eq!(allowlist[0], "example.com");
    }

    #[test]
    fn test_config_without_allowlist() {
        let yaml = r#"
listen_addrs:
  - "0.0.0.0:443"
timeouts:
  connect: 5
  client_hello: 5
  idle: 60
metrics:
  enabled: false
  address: "127.0.0.1:9000"
"#;
        let config = Config::parse(yaml).unwrap();
        assert!(config.allowlist.is_none());
        assert!(!config.metrics.enabled);
    }

    #[test]
    fn test_missing_required_field() {
        let yaml = r#"
listen_addrs:
  - "0.0.0.0:443"
timeouts:
  connect: 5
  idle: 60
metrics:
  enabled: false
  address: "127.0.0.1:9000"
"#;
        let result = Config::parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_yaml() {
        let yaml = "invalid: yaml: content: ::::";
        let result = Config::parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_config() {
        let yaml = "";
        let result = Config::parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_allowlist_exact_match() {
        assert!(matches_allowlist_pattern("example.com", "example.com"));
        assert!(!matches_allowlist_pattern("other.com", "example.com"));
    }

    #[test]
    fn test_allowlist_wildcard_subdomain() {
        assert!(matches_allowlist_pattern(
            "sub.example.com",
            "*.example.com"
        ));
        assert!(matches_allowlist_pattern(
            "deep.sub.example.com",
            "*.example.com"
        ));
        assert!(matches_allowlist_pattern("example.com", "*.example.com"));
        assert!(!matches_allowlist_pattern(
            "example.com.evil.com",
            "*.example.com"
        ));
        assert!(!matches_allowlist_pattern(
            "notexample.com",
            "*.example.com"
        ));
    }

    #[test]
    fn test_allowlist_wildcard_suffix() {
        assert!(matches_allowlist_pattern("test.com", "*test.com"));
        assert!(matches_allowlist_pattern("mytest.com", "*test.com"));
        assert!(!matches_allowlist_pattern("test.org", "*test.com"));
    }

    #[test]
    fn test_allowlist_no_match() {
        assert!(!matches_allowlist_pattern("example.com", "other.com"));
        assert!(!matches_allowlist_pattern("example.com", "*.other.com"));
    }
}
