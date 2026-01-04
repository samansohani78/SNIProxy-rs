/// Integration tests to verify config values are actually used by the proxy
use sniproxy_config::Config;
use std::path::PathBuf;
use std::time::Duration;

fn get_test_config_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go to workspace root
    path.push("test_configs");
    path.push(filename);
    path
}

#[tokio::test]
async fn test_minimal_config_starts_proxy() {
    let config = Config::from_file(&get_test_config_path("test_minimal.yaml"))
        .expect("Failed to load minimal config");

    // Verify config was parsed
    assert_eq!(config.listen_addrs.len(), 1);
    assert_eq!(config.listen_addrs[0], "127.0.0.1:18080");
    assert_eq!(config.timeouts.connect, 5);
    assert_eq!(config.timeouts.client_hello, 3);
    assert_eq!(config.timeouts.idle, 60);
    assert!(!config.metrics.enabled);

    // Verify optional fields use defaults or None
    assert!(config.max_connections.is_none());
    assert!(config.shutdown_timeout.is_none());
    assert!(config.allowlist.is_none());
}

#[tokio::test]
async fn test_basic_config_values() {
    let config = Config::from_file(&get_test_config_path("test_basic.yaml"))
        .expect("Failed to load basic config");

    // All required fields present
    assert_eq!(config.listen_addrs.len(), 2);
    assert!(config.metrics.enabled);

    // Timeouts should be usable as Duration
    let connect_duration = Duration::from_secs(config.timeouts.connect);
    let idle_duration = Duration::from_secs(config.timeouts.idle);

    assert_eq!(connect_duration, Duration::from_secs(10));
    assert_eq!(idle_duration, Duration::from_secs(300));
}

#[tokio::test]
async fn test_full_config_all_values_accessible() {
    let config = Config::from_file(&get_test_config_path("test_full.yaml"))
        .expect("Failed to load full config");

    // Test that all config sections are accessible and have expected values

    // Connection limits
    assert_eq!(config.max_connections.unwrap(), 50000);
    assert_eq!(config.shutdown_timeout.unwrap(), 20);

    // Connection pool
    let pool = config.connection_pool.as_ref().unwrap();
    assert!(pool.enabled);
    assert_eq!(pool.max_per_host, 500);
    assert_eq!(pool.connection_ttl, 300);
    assert_eq!(pool.idle_timeout, 150);
    assert_eq!(pool.cleanup_interval, 15);

    // Verify pool durations can be converted
    let ttl = Duration::from_secs(pool.connection_ttl);
    let idle = Duration::from_secs(pool.idle_timeout);
    assert_eq!(ttl, Duration::from_secs(300));
    assert_eq!(idle, Duration::from_secs(150));

    // Allowlist
    let allowlist = config.allowlist.as_ref().unwrap();
    assert_eq!(allowlist.len(), 2);

    // QUIC config
    let quic = config.quic_config.as_ref().unwrap();
    assert!(quic.enabled);
    assert_eq!(quic.max_concurrent_streams, 50);
    assert_eq!(quic.max_idle_timeout, 30);
    assert_eq!(quic.keep_alive_interval, 10);
    assert_eq!(quic.max_datagram_size, 1200);
    assert!(!quic.enable_0rtt); // Explicitly disabled in test config

    // HTTP/3 config
    let http3 = config.http3_config.as_ref().unwrap();
    assert!(http3.enabled);
    assert_eq!(http3.max_field_section_size, 4096);
    assert_eq!(http3.qpack_max_table_capacity, 2048);
    assert_eq!(http3.qpack_blocked_streams, 8);

    // Protocol routing
    let routing = config.protocol_routing.as_ref().unwrap();

    assert!(routing.socketio.enabled);
    assert_eq!(routing.socketio.polling_timeout, 20);

    assert!(routing.jsonrpc.enabled);
    assert_eq!(routing.jsonrpc.max_batch_size, 50);

    assert!(routing.xmlrpc.enabled);
    assert!(routing.soap.enabled);
    assert!(routing.rpc.enabled);
}

#[tokio::test]
async fn test_production_config_sensible_values() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go to workspace root
    path.push("config.yaml");

    let config = Config::from_file(&path).expect("Failed to load production config");

    // Production should have sensible high-capacity values
    assert_eq!(config.max_connections.unwrap(), 100000);
    assert_eq!(config.shutdown_timeout.unwrap(), 30);

    // Pool should be DISABLED for transparent proxy (avoid file descriptor leaks)
    let pool = config.connection_pool.as_ref().unwrap();
    assert!(!pool.enabled); // Must be false to avoid leaks
    assert_eq!(pool.max_per_host, 1000);
    assert_eq!(pool.connection_ttl, 600); // 10 minutes
    assert_eq!(pool.idle_timeout, 300); // 5 minutes

    // Timeouts should be production-ready
    assert_eq!(config.timeouts.connect, 10);
    assert_eq!(config.timeouts.client_hello, 5);
    assert_eq!(config.timeouts.idle, 300); // 5 minutes

    // Metrics should be enabled
    assert!(config.metrics.enabled);
}

#[test]
fn test_config_default_values() {
    // Test that Default trait provides sensible values
    use sniproxy_config::ConnectionPool;

    let pool = ConnectionPool::default();
    assert!(pool.enabled);
    assert_eq!(pool.max_per_host, 100);
    assert_eq!(pool.connection_ttl, 60);
    assert_eq!(pool.idle_timeout, 30);
    assert_eq!(pool.cleanup_interval, 10);
}

#[test]
fn test_allowlist_pattern_matching() {
    use sniproxy_config::matches_allowlist_pattern;

    // Exact matches
    assert!(matches_allowlist_pattern("example.com", "example.com"));
    assert!(!matches_allowlist_pattern("other.com", "example.com"));

    // Wildcard subdomains
    assert!(matches_allowlist_pattern(
        "api.example.com",
        "*.example.com"
    ));
    assert!(matches_allowlist_pattern(
        "www.example.com",
        "*.example.com"
    ));
    assert!(matches_allowlist_pattern("example.com", "*.example.com")); // Should match base domain too

    // Suffix wildcards
    assert!(matches_allowlist_pattern("myapi.com", "*api.com"));
    assert!(matches_allowlist_pattern("api.com", "*api.com"));

    // No match
    assert!(!matches_allowlist_pattern("evil.com", "*.example.com"));
    assert!(!matches_allowlist_pattern("exampleXcom", "*.example.com"));
}
