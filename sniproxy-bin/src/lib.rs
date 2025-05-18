use std::path::Path;
use std::error::Error;
use std::net::SocketAddr;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper::{Request, Response};
use http_body_util::Full;
use prometheus::{Registry, TextEncoder, Encoder};
use sniproxy_config::Config;
use sniproxy_core::run_proxy;
use tracing_subscriber::{fmt, EnvFilter};
use tokio::net::TcpListener;

pub async fn run(config_path: &Path) -> Result<(), Box<dyn Error>> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive(tracing::Level::INFO.into())
            .add_directive("sniproxy=debug".parse()?)
        )
        .with_target(false)
        .json()
        .init();

    // Load configuration
    let config = Config::from_file(config_path)?;

    // Set up metrics
    let registry = if config.metrics.enabled {
        let registry = Registry::new();
        let metrics_addr: SocketAddr = config.metrics.address.parse()?;
        let metrics_listener = TcpListener::bind(metrics_addr).await?;
        
        let registry_clone = registry.clone();
        tokio::spawn(async move {
            loop {
                if let Ok((stream, _)) = metrics_listener.accept().await {
                    let registry = registry_clone.clone();
                    let io = TokioIo::new(stream);
                    
                    tokio::spawn(async move {
                        let service = hyper::service::service_fn(move |_req: Request<hyper::body::Incoming>| {
                            let registry = registry.clone();
                            async move {
                                let encoder = TextEncoder::new();
                                let metric_families = registry.gather();
                                let mut buffer = vec![];
                                encoder.encode(&metric_families, &mut buffer)
                                    .map_err(|e| format!("Metrics encoding error: {}", e))?;
                                Ok::<_, String>(
                                    Response::new(Full::new(bytes::Bytes::from(buffer)))
                                )
                            }
                        });

                        if let Err(err) = http1::Builder::new()
                            .serve_connection(io, service)
                            .await {
                            eprintln!("Metrics server error: {}", err);
                        }
                    });
                }
            }
        });

        Some(registry)
    } else {
        None
    };

    // Run the proxy
    run_proxy(config, registry).await?;

    Ok(())
}
