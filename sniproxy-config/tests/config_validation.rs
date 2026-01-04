use sniproxy_config::Config;
use std::path::PathBuf;

fn get_test_config_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test_configs");
    path.push(filename);
    path
}

#[test]
fn test_basic_config_loads() {
    let config = Config::from_file(&get_test_config_path("test_basic.yaml"))
        .expect("Failed to load basic config");

    // Verify required fields
    assert_eq!(config.listen_addrs.len(), 2);
    assert_eq!(config.listen_addrs[0], "0.0.0.0:8080");
    assert_eq!(config.listen_addrs[1], "0.0.0.0:8443");

    assert_eq!(config.timeouts.connect, 10);
    assert_eq!(config.timeouts.client_hello, 5);
    assert_eq!(config.timeouts.idle, 300);

    assert!(config.metrics.enabled);
    assert_eq!(config.metrics.address, "0.0.0.0:9091");

    // Optional fields should be None or have defaults
    assert!(config.allowlist.is_none());
    assert!(config.max_connections.is_none());
    assert!(config.shutdown_timeout.is_none());
}

#[test]
fn test_full_config_loads() {
    let config = Config::from_file(&get_test_config_path("test_full.yaml"))
        .expect("Failed to load full config");

    // Required fields
    assert_eq!(config.listen_addrs.len(), 2);
    assert_eq!(config.metrics.address, "0.0.0.0:9091");

    // Optional fields - scalars
    assert_eq!(config.max_connections, Some(50000));
    assert_eq!(config.shutdown_timeout, Some(20));

    // Optional fields - connection pool
    let pool = config
        .connection_pool
        .expect("Connection pool should be configured");
    assert!(pool.enabled);
    assert_eq!(pool.max_per_host, 500);
    assert_eq!(pool.connection_ttl, 300);
    assert_eq!(pool.idle_timeout, 150);
    assert_eq!(pool.cleanup_interval, 15);

    // Optional fields - allowlist
    let allowlist = config.allowlist.expect("Allowlist should be configured");
    assert_eq!(allowlist.len(), 2);
    assert_eq!(allowlist[0], "example.com");
    assert_eq!(allowlist[1], "*.test.com");

    // Optional fields - UDP
    let udp_addrs = config
        .udp_listen_addrs
        .expect("UDP addresses should be configured");
    assert_eq!(udp_addrs.len(), 1);
    assert_eq!(udp_addrs[0], "0.0.0.0:8443");

    // Optional fields - QUIC
    let quic = config.quic_config.expect("QUIC config should be present");
    assert!(quic.enabled);
    assert_eq!(quic.max_concurrent_streams, 50);
    assert_eq!(quic.max_idle_timeout, 30);
    assert_eq!(quic.keep_alive_interval, 10);
    assert_eq!(quic.max_datagram_size, 1200);
    assert!(!quic.enable_0rtt);

    // Optional fields - HTTP/3
    let http3 = config
        .http3_config
        .expect("HTTP/3 config should be present");
    assert!(http3.enabled);
    assert_eq!(http3.max_field_section_size, 4096);
    assert_eq!(http3.qpack_max_table_capacity, 2048);
    assert_eq!(http3.qpack_blocked_streams, 8);

    // Optional fields - Protocol routing
    let routing = config
        .protocol_routing
        .expect("Protocol routing should be present");

    assert!(routing.socketio.enabled);
    assert!(routing.socketio.extract_from_path);
    assert_eq!(routing.socketio.polling_timeout, 20);

    assert!(routing.jsonrpc.enabled);
    assert!(routing.jsonrpc.validate_batch);
    assert_eq!(routing.jsonrpc.max_batch_size, 50);

    assert!(routing.xmlrpc.enabled);
    assert!(routing.xmlrpc.validate_xml);

    assert!(routing.soap.enabled);
    assert!(routing.soap.extract_from_action);
    assert!(!routing.soap.validate_wsdl);

    assert!(routing.rpc.enabled);
    assert!(routing.rpc.detect_from_path);
}

#[test]
fn test_production_config_loads() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up to workspace root
    path.push("config.yaml");

    let config = Config::from_file(&path).expect("Failed to load production config");

    // Verify production config structure
    assert_eq!(config.listen_addrs.len(), 3);
    assert_eq!(config.listen_addrs[0], "0.0.0.0:80");
    assert_eq!(config.listen_addrs[1], "0.0.0.0:443");
    assert_eq!(config.listen_addrs[2], "0.0.0.0:22"); // SSH transparent proxy

    assert_eq!(config.timeouts.connect, 10);
    assert_eq!(config.timeouts.client_hello, 5);
    assert_eq!(config.timeouts.idle, 300);

    assert!(config.metrics.enabled);
    assert_eq!(config.metrics.address, "0.0.0.0:9090");

    assert_eq!(config.max_connections, Some(100000));
    assert_eq!(config.shutdown_timeout, Some(30));

    let pool = config
        .connection_pool
        .expect("Connection pool should be configured");
    assert!(!pool.enabled); // Must be disabled to avoid file descriptor leaks in transparent proxy
    assert_eq!(pool.max_per_host, 1000);
    assert_eq!(pool.connection_ttl, 600);
    assert_eq!(pool.idle_timeout, 300);
    assert_eq!(pool.cleanup_interval, 30);
}

#[test]
fn test_config_with_defaults() {
    let yaml = r#"
listen_addrs:
  - "0.0.0.0:8080"
timeouts:
  connect: 10
  client_hello: 5
  idle: 300
metrics:
  enabled: true
  address: "0.0.0.0:9000"
connection_pool:
  enabled: true
"#;

    let config = Config::parse(yaml).expect("Failed to parse config");

    // Connection pool should use defaults for unspecified values
    let pool = config
        .connection_pool
        .expect("Connection pool should exist");
    assert!(pool.enabled);
    assert_eq!(pool.max_per_host, 100); // default
    assert_eq!(pool.connection_ttl, 60); // default
    assert_eq!(pool.idle_timeout, 30); // default
    assert_eq!(pool.cleanup_interval, 10); // default
}

#[test]
fn test_config_missing_required_field() {
    let yaml = r#"
listen_addrs:
  - "0.0.0.0:8080"
timeouts:
  connect: 10
  idle: 300
metrics:
  enabled: true
  address: "0.0.0.0:9000"
"#;

    let result = Config::parse(yaml);
    assert!(
        result.is_err(),
        "Should fail when client_hello timeout is missing"
    );
}

#[test]
fn test_config_invalid_yaml() {
    let yaml = r#"
listen_addrs: [invalid
timeouts:
  - invalid
"#;

    let result = Config::parse(yaml);
    assert!(result.is_err(), "Should fail on invalid YAML");
}
