use sniproxy_config::{Config, matches_allowlist_pattern};
use sniproxy_core::{SniError, extract_alpn, extract_sni};

#[test]
fn test_config_integration() {
    let config_str = r#"
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
  - "*.example.org"
"#;

    let config = Config::from_str(config_str).expect("Failed to parse config");
    assert_eq!(config.listen_addrs.len(), 2);
    assert_eq!(config.timeouts.connect, 10);
    assert!(config.metrics.enabled);

    // Test allowlist
    let allowlist = config.allowlist.unwrap();
    assert!(matches_allowlist_pattern("example.com", &allowlist[0]));
    assert!(matches_allowlist_pattern("sub.example.org", &allowlist[1]));
    assert!(!matches_allowlist_pattern("example.net", &allowlist[0]));
}

#[test]
fn test_sni_extraction_integration() {
    // Build a complete TLS ClientHello with SNI
    let domain = "integration-test.example.com";
    let domain_bytes = domain.as_bytes();
    let domain_len = domain_bytes.len() as u16;

    let sni_list_len = 3 + domain_len;
    let sni_ext_len = 2 + sni_list_len;
    let extensions_len = 4 + sni_ext_len;
    let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len;
    let record_len = 4 + handshake_len;

    let mut record = vec![
        0x16,
        0x03,
        0x03, // TLS 1.2
        (record_len >> 8) as u8,
        (record_len & 0xff) as u8,
        0x01, // ClientHello
        ((handshake_len as u32) >> 16) as u8,
        (handshake_len >> 8) as u8,
        (handshake_len & 0xff) as u8,
        0x03,
        0x03, // TLS version
    ];
    record.extend_from_slice(&[0; 32]); // Random
    record.extend_from_slice(&[
        0x00, // Session ID length
        0x00,
        0x02, // Cipher suites length
        0x00,
        0x00, // Cipher suite
        0x01,
        0x00, // Compression methods
        (extensions_len >> 8) as u8,
        (extensions_len & 0xff) as u8,
        0x00,
        0x00, // SNI extension type
        (sni_ext_len >> 8) as u8,
        (sni_ext_len & 0xff) as u8,
        (sni_list_len >> 8) as u8,
        (sni_list_len & 0xff) as u8,
        0x00, // Host name type
        (domain_len >> 8) as u8,
        (domain_len & 0xff) as u8,
    ]);
    record.extend_from_slice(domain_bytes);

    let extracted = extract_sni(&record).expect("Failed to extract SNI");
    assert_eq!(extracted, domain);
}

#[test]
fn test_alpn_extraction_integration() {
    // Test with both h2 and http/1.1 in ALPN list
    let protocols: &[&[u8]] = &[b"h2", b"http/1.1"];
    let mut alpn_list = Vec::new();

    for proto in protocols {
        alpn_list.push(proto.len() as u8);
        alpn_list.extend_from_slice(proto);
    }

    let alpn_list_len = alpn_list.len() as u16;
    let alpn_ext_len = 2 + alpn_list_len;
    let extensions_len = 4 + alpn_ext_len;
    let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len;
    let record_len = 4 + handshake_len;

    let mut record = vec![
        0x16,
        0x03,
        0x03,
        (record_len >> 8) as u8,
        (record_len & 0xff) as u8,
        0x01,
        ((handshake_len as u32) >> 16) as u8,
        (handshake_len >> 8) as u8,
        (handshake_len & 0xff) as u8,
        0x03,
        0x03,
    ];
    record.extend_from_slice(&[0; 32]);
    record.extend_from_slice(&[
        0x00,
        0x00,
        0x02,
        0x00,
        0x00,
        0x01,
        0x00,
        (extensions_len >> 8) as u8,
        (extensions_len & 0xff) as u8,
        0x00,
        0x10, // ALPN extension
        (alpn_ext_len >> 8) as u8,
        (alpn_ext_len & 0xff) as u8,
        (alpn_list_len >> 8) as u8,
        (alpn_list_len & 0xff) as u8,
    ]);
    record.extend_from_slice(&alpn_list);

    // Should return the first protocol in the list
    let extracted = extract_alpn(&record).expect("Failed to extract ALPN");
    assert_eq!(extracted, "h2");
}

#[test]
fn test_error_types_integration() {
    // Test invalid TLS version
    let invalid_version = vec![0x16, 0x02, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00];
    match extract_sni(&invalid_version) {
        Err(SniError::InvalidTlsVersion) => {}
        other => panic!("Expected InvalidTlsVersion, got: {:?}", other),
    }

    // Test truncated record
    let truncated = vec![0x16, 0x03];
    match extract_sni(&truncated) {
        Err(SniError::MessageTruncated) => {}
        other => panic!("Expected MessageTruncated, got: {:?}", other),
    }
}

#[test]
fn test_allowlist_patterns_integration() {
    // Test exact matches
    assert!(matches_allowlist_pattern("example.com", "example.com"));
    assert!(!matches_allowlist_pattern("other.com", "example.com"));

    // Test wildcard patterns
    assert!(matches_allowlist_pattern(
        "api.example.com",
        "*.example.com"
    ));
    assert!(matches_allowlist_pattern(
        "v2.api.example.com",
        "*.example.com"
    ));
    assert!(!matches_allowlist_pattern(
        "example.com.evil.net",
        "*.example.com"
    ));

    // Test suffix patterns
    assert!(matches_allowlist_pattern("testapi.com", "*api.com"));
    assert!(matches_allowlist_pattern("api.com", "*api.com"));
}
