/// SNI Proxy with Metrics Example
///
/// This example demonstrates how to run an SNI proxy with Prometheus
/// metrics enabled and domain allowlist configured.
///
/// Run with: cargo run --example proxy_with_metrics
/// View metrics at: http://localhost:9000/metrics

use sniproxy_config::{Config, Timeouts, Metrics};
use sniproxy_core::run_proxy;
use prometheus::Registry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into()))
        .with_target(false)
        .json()
        .init();

    // Create configuration with metrics and allowlist
    let config = Config {
        listen_addrs: vec![
            "0.0.0.0:8080".to_string(),
            "0.0.0.0:8443".to_string(),
        ],
        timeouts: Timeouts {
            connect: 10,
            client_hello: 10,
            idle: 300,
        },
        metrics: Metrics {
            enabled: true,
            address: "127.0.0.1:9000".to_string(),
        },
        // Only allow specific domains
        allowlist: Some(vec![
            "example.com".to_string(),
            "*.example.com".to_string(),
            "api.service.io".to_string(),
        ]),
    };

    println!("Starting SNI Proxy with metrics and allowlist");
    println!("Proxy ports: 8080 (HTTP), 8443 (HTTPS)");
    println!("Metrics available at: http://localhost:9000/metrics");
    println!("Allowed domains:");
    for domain in config.allowlist.as_ref().unwrap() {
        println!("  - {}", domain);
    }
    println!("\nPress Ctrl+C to stop");

    // Create Prometheus registry
    let registry = Registry::new();

    // Run the proxy
    run_proxy(config, Some(registry)).await?;

    Ok(())
}
