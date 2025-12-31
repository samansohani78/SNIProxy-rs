/// Configuration Loading Example
///
/// This example demonstrates various ways to load and validate configuration.
///
/// Run with: cargo run --example config_loading

use sniproxy_config::{Config, matches_allowlist_pattern};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Configuration Loading Example\n");

    // Example 1: Load from YAML string
    println!("1. Loading configuration from YAML string:");
    let yaml_config = r#"
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
  - "*api.service.io"
"#;

    let config = Config::parse(yaml_config)?;
    println!("   ✓ Loaded {} listen addresses", config.listen_addrs.len());
    println!("   ✓ Connect timeout: {}s", config.timeouts.connect);
    println!("   ✓ Metrics enabled: {}", config.metrics.enabled);
    if let Some(ref allowlist) = config.allowlist {
        println!("   ✓ Allowlist entries: {}", allowlist.len());
    }

    // Example 2: Load from file (if exists)
    println!("\n2. Loading configuration from file:");
    let config_path = Path::new("config.yaml");
    if config_path.exists() {
        match Config::from_file(config_path) {
            Ok(file_config) => {
                println!("   ✓ Successfully loaded config.yaml");
                println!("   ✓ Listen addresses: {:?}", file_config.listen_addrs);
            }
            Err(e) => {
                println!("   ✗ Failed to load config.yaml: {}", e);
            }
        }
    } else {
        println!("   ! config.yaml not found (this is OK for the example)");
    }

    // Example 3: Test allowlist pattern matching
    println!("\n3. Testing allowlist pattern matching:");
    let test_cases = vec![
        ("example.com", "example.com", true),
        ("sub.example.com", "*.example.org", false),
        ("api.example.org", "*.example.org", true),
        ("deep.api.example.org", "*.example.org", true),
        ("testapi.service.io", "*api.service.io", true),
        ("wrongapi.service.com", "*api.service.io", false),
    ];

    for (hostname, pattern, expected) in test_cases {
        let result = matches_allowlist_pattern(hostname, pattern);
        let symbol = if result == expected { "✓" } else { "✗" };
        println!("   {} '{}' vs '{}' = {}", symbol, hostname, pattern, result);
    }

    // Example 4: Demonstrate configuration validation
    println!("\n4. Configuration validation:");
    let invalid_yaml = r#"
listen_addrs:
  - "0.0.0.0:80"
timeouts:
  connect: 10
  # Missing client_hello and idle!
metrics:
  enabled: true
  address: "127.0.0.1:9000"
"#;

    match Config::parse(invalid_yaml) {
        Ok(_) => println!("   ✗ Unexpectedly succeeded with invalid config"),
        Err(e) => println!("   ✓ Correctly rejected invalid config: {}", e),
    }

    println!("\nAll examples completed successfully!");
    Ok(())
}
