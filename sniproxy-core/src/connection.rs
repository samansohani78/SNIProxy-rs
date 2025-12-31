use crate::SniError;
use crate::connection_pool::{ConnectionPool, PoolConfig};
use crate::http::{self, HttpError};
use prometheus::{
    HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, Opts, Registry,
};
use sniproxy_config::{Config, matches_allowlist_pattern};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::{Duration, timeout};
use tracing::{debug, error, info, warn};

const MAX_TLS_HEADER_SIZE: usize = 16384; // Increased size for TLS header
const MIN_TLS_HEADER_SIZE: usize = 5; // Minimum size for TLS header
const PEEK_SIZE: usize = 24; // Size to peek for protocol detection (enough for HTTP/2 preface)

const HTTP_METHODS: [&[u8]; 8] = [
    b"GET ",
    b"POST ",
    b"HEAD ",
    b"PUT ",
    b"DELETE ",
    b"OPTIONS ",
    b"PATCH ",
    b"TRACE ",
];

// HTTP/2 detection constants
const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

// TLS ALPN protocol identifiers
const ALPN_HTTP2: &str = "h2";
const ALPN_HTTP3: &[&str] = &["h3", "h3-29", "h3-32"];

// WebSocket detection constants (reserved for future use)
#[allow(dead_code)]
const WEBSOCKET_UPGRADE: &str = "websocket";
#[allow(dead_code)]
const SWITCHING_PROTOCOLS: &[u8] = b"HTTP/1.1 101";

// gRPC detection constants (reserved for future use)
#[allow(dead_code)]
const GRPC_CONTENT_TYPE: &str = "application/grpc";

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Some variants reserved for future use
enum Protocol {
    Http10,    // HTTP/1.0
    Http11,    // HTTP/1.1
    Http2,     // HTTP/2 (ALPN or cleartext)
    Http3,     // HTTP/3 (QUIC)
    WebSocket, // WebSocket over HTTP
    Grpc,      // gRPC over HTTP/2
    Tls,       // TLS without protocol identification
    Unknown,   // Unknown protocol
}

impl Protocol {
    /// Returns a string representation of the protocol for metrics and logging
    #[inline]
    fn as_str(&self) -> &'static str {
        match self {
            Protocol::Http10 => "http1.0",
            Protocol::Http11 => "http1.1",
            Protocol::Http2 => "http2",
            Protocol::Http3 => "http3",
            Protocol::WebSocket => "websocket",
            Protocol::Grpc => "grpc",
            Protocol::Tls => "tls",
            Protocol::Unknown => "unknown",
        }
    }

    /// Returns the default port for this protocol
    #[inline]
    fn default_port(&self) -> u16 {
        match self {
            Protocol::Http10 | Protocol::Http11 | Protocol::WebSocket => 80,
            Protocol::Http2 | Protocol::Grpc | Protocol::Tls => 443,
            Protocol::Http3 => 443,
            Protocol::Unknown => 0,
        }
    }

    /// Returns whether this protocol uses TLS (reserved for future use)
    #[inline]
    #[allow(dead_code)]
    fn is_tls(&self) -> bool {
        matches!(
            self,
            Protocol::Tls | Protocol::Http2 | Protocol::Http3 | Protocol::Grpc
        )
    }

    /// Returns whether this protocol is based on HTTP (reserved for future use)
    #[inline]
    #[allow(dead_code)]
    fn is_http(&self) -> bool {
        matches!(
            self,
            Protocol::Http10
                | Protocol::Http11
                | Protocol::Http2
                | Protocol::Http3
                | Protocol::WebSocket
                | Protocol::Grpc
        )
    }
}

#[derive(Clone)]
pub struct ConnectionHandler {
    config: Arc<Config>,
    metrics: Option<Arc<ConnectionMetrics>>,
    pool: Option<Arc<ConnectionPool>>,
}

struct ConnectionMetrics {
    bytes_transferred: IntCounterVec,
    connections_total: IntCounterVec,
    connections_active: IntGauge,
    #[allow(dead_code)] // Reserved for future per-connection duration tracking
    connection_duration: HistogramVec,
    errors_total: IntCounterVec,
    protocol_distribution: IntCounterVec,
}

impl ConnectionMetrics {
    fn new(registry: &Registry) -> Self {
        let bytes_transferred = IntCounterVec::new(
            Opts::new(
                "sniproxy_bytes_transferred_total",
                "Total bytes transferred per host and direction",
            ),
            &["host", "direction"],
        )
        .unwrap();
        registry
            .register(Box::new(bytes_transferred.clone()))
            .unwrap();

        let connections_total = IntCounterVec::new(
            Opts::new(
                "sniproxy_connections_total",
                "Total number of connections handled",
            ),
            &["protocol", "status"],
        )
        .unwrap();
        registry
            .register(Box::new(connections_total.clone()))
            .unwrap();

        let connections_active = IntGauge::new(
            "sniproxy_connections_active",
            "Number of currently active connections",
        )
        .unwrap();
        registry
            .register(Box::new(connections_active.clone()))
            .unwrap();

        let connection_duration = HistogramVec::new(
            HistogramOpts::new(
                "sniproxy_connection_duration_seconds",
                "Connection duration in seconds",
            )
            .buckets(vec![
                0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0,
            ]),
            &["protocol", "host"],
        )
        .unwrap();
        registry
            .register(Box::new(connection_duration.clone()))
            .unwrap();

        let errors_total = IntCounterVec::new(
            Opts::new("sniproxy_errors_total", "Total number of errors by type"),
            &["error_type", "protocol"],
        )
        .unwrap();
        registry.register(Box::new(errors_total.clone())).unwrap();

        let protocol_distribution = IntCounterVec::new(
            Opts::new(
                "sniproxy_protocol_distribution_total",
                "Distribution of detected protocols",
            ),
            &["protocol"],
        )
        .unwrap();
        registry
            .register(Box::new(protocol_distribution.clone()))
            .unwrap();

        Self {
            bytes_transferred,
            connections_total,
            connections_active,
            connection_duration,
            errors_total,
            protocol_distribution,
        }
    }
}

impl ConnectionHandler {
    pub fn new(config: Arc<Config>, registry: Option<&Registry>) -> Self {
        let metrics = registry.map(|r| Arc::new(ConnectionMetrics::new(r)));

        // Initialize connection pool if configured
        let pool = if let Some(pool_config) = &config.connection_pool {
            let pool_cfg = PoolConfig {
                enabled: pool_config.enabled,
                max_per_host: pool_config.max_per_host,
                connection_ttl: pool_config.connection_ttl,
                idle_timeout: pool_config.idle_timeout,
            };

            let pool = if let Some(reg) = registry {
                ConnectionPool::with_metrics(pool_cfg, reg).ok()
            } else {
                Some(ConnectionPool::new(pool_cfg))
            };

            pool.map(Arc::new)
        } else {
            None
        };

        Self {
            config,
            metrics,
            pool,
        }
    }

    pub async fn handle_connection(&self, mut client: TcpStream, client_addr: SocketAddr) {
        let peer = client_addr.to_string();
        let start_time = std::time::Instant::now();

        // Track active connections
        if let Some(ref metrics) = self.metrics {
            metrics.connections_active.inc();
        }

        info!(peer, "New connection");

        let result = self.process_connection(&mut client, client_addr).await;
        let duration = start_time.elapsed().as_secs_f64();

        // Update metrics
        if let Some(ref metrics) = self.metrics {
            metrics.connections_active.dec();

            let status = if result.is_ok() { "success" } else { "failure" };
            metrics
                .connections_total
                .with_label_values(&["unknown", status])
                .inc();
        }

        match result {
            Ok(_) => info!(peer, duration_secs = %duration, "Connection completed"),
            Err(e) => {
                let error_msg = e.to_string();

                // Only log as ERROR if it's a real problem, not client misbehavior
                let is_client_error = error_msg.contains("HTTP/2 frame")
                    || error_msg.contains("timeout")
                    || error_msg.contains("ClientHello")
                    || error_msg.contains("Host header")
                    || error_msg.contains("Unknown protocol")
                    || error_msg.contains("Connection reset")
                    || error_msg.contains("Broken pipe");

                if is_client_error {
                    // Client sent invalid/malformed request - debug level only
                    debug!(peer, error = %error_msg, duration_secs = %duration, "Client request rejected");
                } else {
                    // Real error - log at error level
                    error!(peer, error = %error_msg, duration_secs = %duration, "Connection error");
                }

                if let Some(ref metrics) = self.metrics {
                    metrics
                        .errors_total
                        .with_label_values(&["connection", "unknown"])
                        .inc();
                }
            }
        }
    }

    /// Helper function to peek at the beginning of a TCP stream with timeout
    #[inline]
    async fn peek_bytes(&self, client: &mut TcpStream, size: usize) -> io::Result<Vec<u8>> {
        let mut peek_buf = vec![0u8; size];
        let hello_timeout = Duration::from_secs(self.config.timeouts.client_hello);

        let n = timeout(hello_timeout, client.peek(&mut peek_buf)).await??;
        peek_buf.truncate(n);

        Ok(peek_buf)
    }

    /// Detects HTTP/1.x version from a request line
    #[inline]
    fn detect_http_version(&self, bytes: &[u8]) -> Protocol {
        if let Ok(line) = std::str::from_utf8(bytes) {
            if line.contains("HTTP/1.0") {
                return Protocol::Http10;
            } else if line.contains("HTTP/1.1") {
                return Protocol::Http11;
            }
        }
        // Default to HTTP/1.1 if we can't determine the version
        Protocol::Http11
    }

    async fn process_connection(
        &self,
        client: &mut TcpStream,
        addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Peek enough bytes to identify the protocol (including HTTP/2 preface)
        let peek_buf = self.peek_bytes(client, PEEK_SIZE).await?;
        if peek_buf.is_empty() {
            return Err("Empty connection".into());
        }

        // Determine the protocol based on the peeked bytes
        let protocol = self.detect_protocol(&peek_buf).await?;
        debug!("Detected protocol: {}", protocol.as_str());

        // Record protocol distribution metric
        if let Some(ref metrics) = self.metrics {
            metrics
                .protocol_distribution
                .with_label_values(&[protocol.as_str()])
                .inc();
        }

        // Handle the connection based on the detected protocol
        match protocol {
            Protocol::Http10 | Protocol::Http11 => self.handle_http(client, protocol).await?,
            Protocol::Http2 => {
                if peek_buf[0] == 0x16 {
                    // HTTP/2 over TLS
                    self.handle_https(client, Some(protocol)).await?
                } else {
                    // HTTP/2 cleartext (h2c)
                    self.handle_http2_cleartext(client).await?
                }
            }
            Protocol::WebSocket => self.handle_http(client, protocol).await?,
            Protocol::Grpc => self.handle_http2(client, true).await?,
            Protocol::Tls => self.handle_https(client, None).await?,
            Protocol::Http3 => {
                // HTTP/3 requires QUIC which we'd handle differently
                // For now, we'll just handle the TLS part
                self.handle_https(client, Some(protocol)).await?
            }
            Protocol::Unknown => {
                // Log first 64 bytes for debugging unknown protocols
                let preview_len = peek_buf.len().min(64);
                let hex_preview: String = peek_buf[..preview_len]
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ");

                let ascii_preview = String::from_utf8_lossy(&peek_buf[..preview_len]);

                warn!(
                    peer = %addr,
                    bytes = preview_len,
                    hex = %hex_preview,
                    ascii = %ascii_preview,
                    "Unknown protocol detected - proxy requires SNI (TLS) or Host header (HTTP)"
                );

                return Err(
                    "Unknown protocol - SNIProxy requires SNI (TLS) or Host header (HTTP)".into(),
                );
            }
        }

        Ok(())
    }

    /// Detects the protocol based on the first bytes of the connection
    async fn detect_protocol(
        &self,
        peek_buf: &[u8],
    ) -> Result<Protocol, Box<dyn std::error::Error>> {
        // Check for HTTP/2 cleartext preface first (it's very distinctive)
        if peek_buf.len() >= HTTP2_PREFACE.len()
            && &peek_buf[..HTTP2_PREFACE.len()] == HTTP2_PREFACE
        {
            debug!("Detected HTTP/2 cleartext preface");
            return Ok(Protocol::Http2);
        }

        // Check for HTTP methods (HTTP/1.x)
        for method in &HTTP_METHODS {
            if peek_buf.starts_with(method) {
                debug!("Found HTTP method: {:?}", String::from_utf8_lossy(method));

                // Try to find the end of the first line to determine HTTP version
                if let Some(pos) = peek_buf.iter().position(|&b| b == b'\n') {
                    let first_line = &peek_buf[..pos];
                    let http_version = self.detect_http_version(first_line);
                    debug!("Detected HTTP version: {}", http_version.as_str());
                    return Ok(http_version);
                }

                // If we can't find the end of the line, default to HTTP/1.1
                return Ok(Protocol::Http11);
            }
        }

        // Check for TLS handshake
        if !peek_buf.is_empty() && peek_buf[0] == 0x16 {
            debug!("Found TLS handshake marker");
            // We'll identify specific TLS protocol (HTTP/2, HTTP/3) during the TLS handshake
            return Ok(Protocol::Tls);
        }

        debug!(
            "Unknown protocol, first bytes: {:02x?}",
            &peek_buf[..peek_buf.len().min(8)]
        );
        Ok(Protocol::Unknown)
    }

    async fn handle_http(
        &self,
        client: &mut TcpStream,
        protocol: Protocol,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = Vec::with_capacity(16384); // Increased capacity

        // Extract host from HTTP headers
        let (host, bytes_read) = match http::extract_host(client, &mut buffer).await {
            Ok(result) => result,
            Err(HttpError::NoHostHeader) => {
                warn!("No Host header in HTTP request");
                return Ok(());
            }
            Err(e) => return Err(Box::new(e)),
        };

        debug!(
            host,
            protocol = protocol.as_str(),
            "Extracted Host from HTTP headers"
        );

        // Check allowlist if configured
        if let Some(ref allowlist) = self.config.allowlist
            && !self.is_host_allowed(&host, allowlist)
        {
            warn!(host, "Host not in allowlist");
            return Ok(());
        }

        // Setup metrics if enabled
        let metrics = self.metrics.as_ref().map(|m| {
            let host_protocol = format!("{}-{}", host, protocol.as_str());
            // Static string references for direction labels
            const TX: &str = "tx";
            const RX: &str = "rx";
            (
                m.bytes_transferred
                    .with_label_values(&[host_protocol.as_str(), TX]),
                m.bytes_transferred
                    .with_label_values(&[host_protocol.as_str(), RX]),
            )
        });

        // Parse host and port (Host header may include port like "example.com:8080")
        let (hostname, port) = if let Some(colon_pos) = host.rfind(':') {
            // Check if the part after colon is a valid port number
            if let Ok(p) = host[colon_pos + 1..].parse::<u16>() {
                (host[..colon_pos].to_string(), p)
            } else {
                // Not a valid port, treat entire string as hostname
                (host.clone(), protocol.default_port())
            }
        } else {
            // No port specified, use default
            (host.clone(), protocol.default_port())
        };

        // Tunnel the connection
        match protocol {
            Protocol::WebSocket => {
                // For WebSockets, we need to monitor the upgrade
                http::tunnel_websocket(client, &buffer[..bytes_read], &hostname, port, metrics)
                    .await?
            }
            _ => {
                // Standard HTTP tunneling
                http::tunnel_http(client, &buffer[..bytes_read], &hostname, port, metrics).await?
            }
        }

        Ok(())
    }

    async fn handle_http2_cleartext(
        &self,
        client: &mut TcpStream,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // For h2c, we need to extract the host from the HTTP/2 headers
        // This requires parsing the HTTP/2 frames

        // Read the preface (we already peeked at it, but now we need to consume it)
        let mut preface_buffer = vec![0u8; HTTP2_PREFACE.len()];
        client.read_exact(&mut preface_buffer).await?;

        // Extract :authority pseudo-header from HTTP/2 HEADERS frame
        let (host, headers_frame) = match http::extract_http2_authority(client).await {
            Ok((authority, frame_data)) => {
                debug!(
                    authority = authority,
                    protocol = "http2",
                    "Extracted :authority from HTTP/2 HEADERS frame"
                );
                (authority, frame_data)
            }
            Err(e) => {
                // Don't log as error - many clients send malformed HTTP/2 probes
                debug!("Invalid HTTP/2 frame from client: {}", e);
                return Ok(()); // Close connection gracefully
            }
        };

        // Check allowlist if configured
        if let Some(ref allowlist) = self.config.allowlist
            && !self.is_host_allowed(&host, allowlist)
        {
            warn!(host, "Host not in allowlist");
            return Ok(());
        }

        // Setup metrics if enabled
        let metrics = self.metrics.as_ref().map(|m| {
            let host_protocol = format!("{}-http2", host);
            // Static string references for direction labels
            const TX: &str = "tx";
            const RX: &str = "rx";
            (
                m.bytes_transferred
                    .with_label_values(&[host_protocol.as_str(), TX]),
                m.bytes_transferred
                    .with_label_values(&[host_protocol.as_str(), RX]),
            )
        });

        // Connect to the target server
        let target_addr = format!("{}:80", host); // HTTP/2 cleartext typically uses port 80
        let mut server = self.connect_to_server(&target_addr).await?;

        // Send the HTTP/2 preface and HEADERS frame to the server
        server.write_all(&preface_buffer).await?;
        server.write_all(&headers_frame).await?;

        // Start bidirectional copy
        let idle_timeout = Duration::from_secs(self.config.timeouts.idle);
        copy_bidirectional_timeout(client, server, idle_timeout, metrics).await?;

        Ok(())
    }

    async fn handle_http2(
        &self,
        client: &mut TcpStream,
        is_grpc: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // This is similar to handle_http2_cleartext but with gRPC-specific handling

        // Read the preface (we already peeked at it, but now we need to consume it)
        let mut buffer = vec![0u8; HTTP2_PREFACE.len()];
        client.read_exact(&mut buffer).await?;

        // For gRPC, we might want to extract additional headers or do specific handling
        let host = if is_grpc {
            // For gRPC, try to extract the authority from headers
            // Placeholder until we implement full HTTP/2 frame parsing
            "grpc.service".to_string()
        } else {
            "default.host".to_string()
        };

        debug!(
            host,
            protocol = if is_grpc { "grpc" } else { "http2" },
            "Extracted host"
        );

        // Check allowlist if configured
        if let Some(ref allowlist) = self.config.allowlist
            && !self.is_host_allowed(&host, allowlist)
        {
            warn!(host, "Host not in allowlist");
            return Ok(());
        }

        // Setup metrics if enabled
        let metrics = self.metrics.as_ref().map(|m| {
            let protocol = if is_grpc { "grpc" } else { "http2" };
            let host_protocol = format!("{}-{}", host, protocol);
            let tx_label = String::from("tx");
            let rx_label = String::from("rx");
            (
                m.bytes_transferred
                    .with_label_values(&[&host_protocol, &tx_label]),
                m.bytes_transferred
                    .with_label_values(&[&host_protocol, &rx_label]),
            )
        });

        // Connect to the target server
        let default_port = if is_grpc { 443 } else { 80 }; // gRPC typically uses TLS
        let target_addr = format!("{}:{}", host, default_port);
        let mut server = self.connect_to_server(&target_addr).await?;

        // Send the HTTP/2 preface to the server
        server.write_all(&buffer).await?;

        // Start bidirectional copy
        let idle_timeout = Duration::from_secs(self.config.timeouts.idle);
        copy_bidirectional_timeout(client, server, idle_timeout, metrics).await?;

        Ok(())
    }

    /// Helper method to connect to a server with timeout
    async fn connect_to_server(
        &self,
        target_addr: &str,
    ) -> Result<TcpStream, Box<dyn std::error::Error>> {
        // Try to get connection from pool first
        if let Some(ref pool) = self.pool
            && let Some(stream) = pool.get(target_addr).await
        {
            debug!("Using pooled connection to {}", target_addr);
            return Ok(stream);
        }

        // No pooled connection available, create new one
        debug!("Resolving target address: {}", target_addr);
        let addr = lookup_host(target_addr)
            .await?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Failed to resolve target"))?;

        let connect_timeout = Duration::from_secs(self.config.timeouts.connect);
        debug!("Connecting to target: {}", addr);
        let server = timeout(connect_timeout, TcpStream::connect(addr)).await??;

        Ok(server)
    }

    /// Return a connection to the pool if pooling is enabled
    /// Reserved for future use with HTTP/1.1 keep-alive support
    #[allow(dead_code)]
    async fn return_to_pool(&self, target_addr: String, stream: TcpStream) {
        if let Some(ref pool) = self.pool {
            if pool.put(target_addr, stream).await {
                debug!("Connection returned to pool");
            } else {
                debug!("Connection not returned to pool (pool full or disabled)");
            }
        }
    }

    /// Mark a connection as inactive in the pool (if pooling is enabled)
    /// Reserved for future use with HTTP/1.1 keep-alive support
    #[allow(dead_code)]
    fn mark_connection_inactive(&self) {
        if let Some(ref pool) = self.pool {
            pool.mark_inactive();
        }
    }

    async fn handle_https(
        &self,
        client: &mut TcpStream,
        detected_protocol: Option<Protocol>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hello_timeout = Duration::from_secs(self.config.timeouts.client_hello);
        let mut reader = BufReader::new(client);

        // Read and verify TLS header (5 bytes)
        let mut record = Vec::with_capacity(16384);
        record.resize(MIN_TLS_HEADER_SIZE, 0);

        debug!("Reading TLS header...");
        timeout(
            hello_timeout,
            reader.read_exact(&mut record[..MIN_TLS_HEADER_SIZE]),
        )
        .await??;

        // Verify it's a TLS handshake
        if record[0] != 0x16 {
            debug!("Not a TLS handshake, first byte: {:02x}", record[0]);
            return Err("Not a TLS handshake".into());
        }

        // Get record length and validate
        let record_length = ((record[3] as usize) << 8) | (record[4] as usize);
        debug!("TLS record length: {}", record_length);

        if !(4..=MAX_TLS_HEADER_SIZE).contains(&record_length) {
            debug!("Invalid TLS record length: {}", record_length);
            return Err("Invalid TLS record length".into());
        }

        // Read the rest of the record
        record.resize(MIN_TLS_HEADER_SIZE + record_length, 0);
        debug!("Reading TLS record body ({} bytes)...", record_length);
        timeout(
            hello_timeout,
            reader.read_exact(&mut record[MIN_TLS_HEADER_SIZE..]),
        )
        .await??;

        // Extract SNI and ALPN (if available)
        debug!("Record complete, total length: {}", record.len());
        let sni = crate::extract_sni(&record)?;
        let alpn = crate::extract_alpn(&record);

        // Determine protocol based on ALPN if not already detected
        let protocol = match detected_protocol {
            Some(p) => p,
            None => {
                if let Some(proto) = alpn {
                    debug!(sni, alpn = proto, "Extracted ALPN from ClientHello");
                    if proto == ALPN_HTTP2 {
                        Protocol::Http2
                    } else if ALPN_HTTP3.contains(&proto) {
                        Protocol::Http3
                    } else {
                        Protocol::Tls
                    }
                } else {
                    Protocol::Tls
                }
            }
        };

        debug!(
            sni,
            protocol = protocol.as_str(),
            "Extracted SNI from ClientHello"
        );

        // Check allowlist if configured
        if let Some(ref allowlist) = self.config.allowlist
            && !self.is_host_allowed(&sni, allowlist)
        {
            warn!(sni, "Host not in allowlist");
            return Err(Box::new(SniError::InvalidSniFormat));
        }

        // Resolve and connect to target
        let target_addr = format!("{}:443", sni);
        debug!("Resolving target address: {}", target_addr);
        let addr = lookup_host(&target_addr)
            .await?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Failed to resolve target"))?;

        let connect_timeout = Duration::from_secs(self.config.timeouts.connect);
        debug!("Connecting to target: {}", addr);
        let mut server = timeout(connect_timeout, TcpStream::connect(addr)).await??;

        // Setup metrics if enabled
        let metrics = self.metrics.as_ref().map(|m| {
            let host_protocol = format!("{}-{}", sni, protocol.as_str());
            let tx_label = String::from("tx");
            let rx_label = String::from("rx");
            (
                m.bytes_transferred
                    .with_label_values(&[&host_protocol, &tx_label]),
                m.bytes_transferred
                    .with_label_values(&[&host_protocol, &rx_label]),
            )
        });

        // Send the captured ClientHello
        debug!("Sending ClientHello to target");
        server.write_all(&record).await?;

        // Get the underlying TcpStream back from the BufReader
        let client = reader.into_inner();

        // Begin bidirectional copy with timeout
        debug!("Starting bidirectional tunnel for {}", sni);
        let idle_timeout = Duration::from_secs(self.config.timeouts.idle);
        copy_bidirectional_timeout(client, server, idle_timeout, metrics).await?;

        debug!("HTTPS connection completed successfully");
        Ok(())
    }

    fn is_host_allowed(&self, host: &str, allowlist: &[String]) -> bool {
        // Special case: "*" allows all hosts
        if allowlist.contains(&"*".to_string()) {
            return true;
        }

        let host_lower = host.to_lowercase();
        allowlist
            .iter()
            .any(|pattern| matches_allowlist_pattern(&host_lower, &pattern.to_lowercase()))
    }
}

async fn copy_bidirectional_timeout<T, U>(
    client: T,
    server: U,
    idle_timeout: Duration,
    metrics: Option<(IntCounter, IntCounter)>,
) -> io::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let (mut client_read, mut client_write) = io::split(client);
    let (mut server_read, mut server_write) = io::split(server);

    let client_to_server = async {
        let mut buf = [0u8; 8192];
        loop {
            let n = timeout(idle_timeout, client_read.read(&mut buf)).await??;
            if n == 0 {
                break;
            }
            server_write.write_all(&buf[..n]).await?;
            if let Some((counter, _)) = &metrics {
                counter.inc_by(n as u64);
            }
        }
        server_write.shutdown().await?;
        Ok::<_, io::Error>(())
    };

    let server_to_client = async {
        let mut buf = [0u8; 8192];
        loop {
            let n = timeout(idle_timeout, server_read.read(&mut buf)).await??;
            if n == 0 {
                break;
            }
            client_write.write_all(&buf[..n]).await?;
            if let Some((_, counter)) = &metrics {
                counter.inc_by(n as u64);
            }
        }
        client_write.shutdown().await?;
        Ok::<_, io::Error>(())
    };

    tokio::try_join!(client_to_server, server_to_client)?;
    Ok(())
}
