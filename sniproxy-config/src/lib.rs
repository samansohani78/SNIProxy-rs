use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// SNIProxy configuration loaded from YAML.
///
/// This structure defines all configuration options for the proxy server including
/// listen addresses, timeout settings, metrics configuration, and domain allowlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Protocol routing configuration for web protocols (optional)
    #[serde(default)]
    pub protocol_routing: Option<ProtocolRouting>,
    /// UDP listener addresses for HTTP/3 and QUIC (optional)
    #[serde(default)]
    pub udp_listen_addrs: Option<Vec<String>>,
    /// QUIC protocol configuration (optional)
    #[serde(default)]
    pub quic_config: Option<QuicConfig>,
    /// HTTP/3 configuration (optional)
    #[serde(default)]
    pub http3_config: Option<Http3Config>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeouts {
    /// Maximum time to establish backend connection (default: 10s)
    pub connect: u64,
    /// Maximum time to receive TLS ClientHello or HTTP headers (default: 10s)
    pub client_hello: u64,
    /// Maximum idle time for established connections (default: 300s)
    pub idle: u64,
}

/// Prometheus metrics server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let config = serde_yaml_ng::from_str(&contents)?;
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
        let config = serde_yaml_ng::from_str(contents)?;
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

/// Protocol routing configuration for web protocols
///
/// Optional configuration to enable/disable specific web protocol detection
/// and configure protocol-specific settings.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ProtocolRouting {
    /// Socket.IO configuration
    #[serde(default)]
    pub socketio: SocketIOConfig,
    /// JSON-RPC configuration
    #[serde(default)]
    pub jsonrpc: JsonRpcConfig,
    /// XML-RPC configuration
    #[serde(default)]
    pub xmlrpc: XmlRpcConfig,
    /// SOAP configuration
    #[serde(default)]
    pub soap: SoapConfig,
    /// Generic RPC configuration
    #[serde(default)]
    pub rpc: RpcConfig,
}

/// Socket.IO protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SocketIOConfig {
    /// Enable Socket.IO detection (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Extract namespace from path (default: true)
    #[serde(default = "default_true")]
    pub extract_from_path: bool,
    /// Polling timeout in seconds (default: 30)
    #[serde(default = "default_polling_timeout")]
    pub polling_timeout: u64,
}

impl Default for SocketIOConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extract_from_path: true,
            polling_timeout: 30,
        }
    }
}

/// JSON-RPC protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcConfig {
    /// Enable JSON-RPC detection (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Validate batch requests (default: true)
    #[serde(default = "default_true")]
    pub validate_batch: bool,
    /// Maximum batch size (default: 100)
    #[serde(default = "default_max_batch_size")]
    pub max_batch_size: usize,
}

impl Default for JsonRpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            validate_batch: true,
            max_batch_size: 100,
        }
    }
}

/// XML-RPC protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct XmlRpcConfig {
    /// Enable XML-RPC detection (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Validate XML structure (default: true)
    #[serde(default = "default_true")]
    pub validate_xml: bool,
}

impl Default for XmlRpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            validate_xml: true,
        }
    }
}

/// SOAP protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoapConfig {
    /// Enable SOAP detection (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Extract SOAPAction from headers (default: true)
    #[serde(default = "default_true")]
    pub extract_from_action: bool,
    /// Validate WSDL (default: false, reserved for future use)
    #[serde(default = "default_false")]
    pub validate_wsdl: bool,
}

impl Default for SoapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extract_from_action: true,
            validate_wsdl: false,
        }
    }
}

/// Generic RPC protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RpcConfig {
    /// Enable RPC detection (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Detect from path patterns (default: true)
    #[serde(default = "default_true")]
    pub detect_from_path: bool,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            detect_from_path: true,
        }
    }
}

// Default value helpers
fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_polling_timeout() -> u64 {
    30
}

fn default_max_batch_size() -> usize {
    100
}

/// QUIC protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QuicConfig {
    /// Enable QUIC support (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum concurrent bidirectional streams per connection (default: 100)
    #[serde(default = "default_max_concurrent_streams")]
    pub max_concurrent_streams: u32,
    /// Maximum idle timeout in seconds (default: 60)
    #[serde(default = "default_max_idle_timeout")]
    pub max_idle_timeout: u64,
    /// Keep-alive interval in seconds (default: 15)
    #[serde(default = "default_keep_alive_interval")]
    pub keep_alive_interval: u64,
    /// Maximum datagram size in bytes (default: 1350 for MTU safety)
    #[serde(default = "default_max_datagram_size")]
    pub max_datagram_size: usize,
    /// Enable 0-RTT resumption (default: true)
    #[serde(default = "default_true")]
    pub enable_0rtt: bool,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_streams: default_max_concurrent_streams(),
            max_idle_timeout: default_max_idle_timeout(),
            keep_alive_interval: default_keep_alive_interval(),
            max_datagram_size: default_max_datagram_size(),
            enable_0rtt: true,
        }
    }
}

/// HTTP/3 protocol configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Http3Config {
    /// Enable HTTP/3 support (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum HTTP header field section size in bytes (default: 8192)
    #[serde(default = "default_max_field_section_size")]
    pub max_field_section_size: usize,
    /// QPACK maximum table capacity (default: 4096)
    #[serde(default = "default_qpack_max_table_capacity")]
    pub qpack_max_table_capacity: usize,
    /// QPACK maximum blocked streams (default: 16)
    #[serde(default = "default_qpack_blocked_streams")]
    pub qpack_blocked_streams: u16,
}

impl Default for Http3Config {
    fn default() -> Self {
        Self {
            enabled: true,
            max_field_section_size: default_max_field_section_size(),
            qpack_max_table_capacity: default_qpack_max_table_capacity(),
            qpack_blocked_streams: default_qpack_blocked_streams(),
        }
    }
}

// QUIC default value helpers
fn default_max_concurrent_streams() -> u32 {
    100
}

fn default_max_idle_timeout() -> u64 {
    60
}

fn default_keep_alive_interval() -> u64 {
    15
}

fn default_max_datagram_size() -> usize {
    1350
}

// HTTP/3 default value helpers
fn default_max_field_section_size() -> usize {
    8192
}

fn default_qpack_max_table_capacity() -> usize {
    4096
}

fn default_qpack_blocked_streams() -> u16 {
    16
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
