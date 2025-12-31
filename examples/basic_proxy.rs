/// Basic SNI Proxy Example
///
/// This example demonstrates how to run a basic SNI proxy server
/// with minimal configuration.
///
/// Run with: cargo run --example basic_proxy

use sniproxy_config::{Config, Timeouts, Metrics};
use sniproxy_core::run_proxy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into()))
        .init();

    // Create configuration programmatically
    let config = Config {
        listen_addrs: vec![
            "0.0.0.0:8080".to_string(),   // HTTP
            "0.0.0.0:8443".to_string(),   // HTTPS
        ],
        timeouts: Timeouts {
            connect: 10,
            client_hello: 10,
            idle: 300,
        },
        metrics: Metrics {
            enabled: false,
            address: "127.0.0.1:9000".to_string(),
        },
        allowlist: None,  // Allow all domains
    };

    println!("Starting SNI Proxy on ports 8080 (HTTP) and 8443 (HTTPS)");
    println!("Press Ctrl+C to stop");

    // Run the proxy
    run_proxy(config, None).await?;

    Ok(())
}
