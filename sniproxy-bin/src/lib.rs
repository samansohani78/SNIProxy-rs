use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use prometheus::{Encoder, Registry, TextEncoder};
use sniproxy_config::Config;
use sniproxy_core::run_proxy;
use std::error::Error;
use std::net::SocketAddr;
use std::path::Path;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

pub async fn run(config_path: &Path) -> Result<(), Box<dyn Error>> {
    // Initialize logging
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
                .add_directive("sniproxy=debug".parse()?),
        )
        .with_target(false)
        .json()
        .init();

    // Load configuration
    let config = Config::from_file(config_path)?;

    // Create shutdown channel for coordinating graceful shutdown
    let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

    // Set up metrics with proper cleanup
    let (registry, metrics_handle) = if config.metrics.enabled {
        let registry = Registry::new();
        let metrics_addr: SocketAddr = config.metrics.address.parse()?;
        let metrics_listener = TcpListener::bind(metrics_addr).await?;
        info!("Metrics server listening on {}", metrics_addr);

        let registry_clone = registry.clone();
        let mut shutdown_rx_clone = shutdown_rx.resubscribe();

        // Spawn metrics server with shutdown coordination
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Check for shutdown signal
                    _ = shutdown_rx_clone.recv() => {
                        info!("Metrics server shutting down");
                        break;
                    }
                    // Accept connections
                    result = metrics_listener.accept() => {
                        if let Ok((stream, _)) = result {
                            let registry = registry_clone.clone();
                            let io = TokioIo::new(stream);

                            tokio::spawn(async move {
                                let service = hyper::service::service_fn(
                                    move |req: Request<hyper::body::Incoming>| {
                                        let registry = registry.clone();
                                        async move {
                                            match req.uri().path() {
                                                "/metrics" => {
                                                    // Serve Prometheus metrics
                                                    let encoder = TextEncoder::new();
                                                    let metric_families = registry.gather();
                                                    let mut buffer = vec![];
                                                    encoder.encode(&metric_families, &mut buffer).map_err(
                                                        |e| format!("Metrics encoding error: {}", e),
                                                    )?;
                                                    Ok::<_, String>(Response::new(Full::new(
                                                        bytes::Bytes::from(buffer),
                                                    )))
                                                }
                                                "/health" => {
                                                    // Health check endpoint
                                                    let health_response =
                                                        r#"{"status":"healthy","service":"sniproxy"}"#;
                                                    Ok::<_, String>(Response::new(Full::new(
                                                        bytes::Bytes::from(health_response),
                                                    )))
                                                }
                                                "/" => {
                                                    // Root endpoint - show available endpoints
                                                    let index_response =
                                                        r#"{"endpoints":["/health","/metrics"]}"#;
                                                    Ok::<_, String>(Response::new(Full::new(
                                                        bytes::Bytes::from(index_response),
                                                    )))
                                                }
                                                _ => {
                                                    // 404 for unknown paths
                                                    let not_found = r#"{"error":"not_found"}"#;
                                                    Ok::<_, String>(Response::new(Full::new(
                                                        bytes::Bytes::from(not_found),
                                                    )))
                                                }
                                            }
                                        }
                                    },
                                );

                                if let Err(err) = http1::Builder::new().serve_connection(io, service).await
                                {
                                    warn!("Metrics server connection error: {}", err);
                                }
                            });
                        }
                    }
                }
            }
        });

        (Some(registry), Some(handle))
    } else {
        (None, None)
    };

    // Run the proxy with shutdown coordination
    let proxy_result = run_proxy(config, registry, shutdown_rx).await;

    // Signal shutdown to metrics server
    let _ = shutdown_tx.send(());

    // Wait for metrics server to finish
    if let Some(handle) = metrics_handle {
        info!("Waiting for metrics server to shut down");
        let _ = handle.await;
    }

    proxy_result
}
