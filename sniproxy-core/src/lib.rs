pub mod connection;
pub mod connection_pool;
mod http;
pub mod metrics_cache;
pub mod protocols;
pub mod quic_handler;
pub mod udp_connection;

use connection::ConnectionHandler;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use prometheus::Registry;
use sniproxy_config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio::signal;
use tokio::sync::{Semaphore, broadcast};
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::udp_connection::UdpConnectionHandler;

/// Runs the SNI proxy server with the given configuration.
///
/// This function creates TCP listeners on all configured addresses and starts
/// accepting connections. Each connection is handled in a separate task, allowing
/// for high concurrency. The function implements graceful shutdown with connection
/// tracking and connection limits.
///
/// # Arguments
///
/// * `config` - The proxy configuration including listen addresses, timeouts, and connection limits
/// * `registry` - Optional Prometheus registry for metrics collection
/// * `shutdown_rx` - Broadcast receiver for shutdown coordination across all components
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown (via Ctrl+C or shutdown signal), or an error if listener setup fails.
///
/// # Examples
///
/// ```no_run
/// use sniproxy_core::run_proxy;
/// use sniproxy_config::Config;
/// use tokio::sync::broadcast;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = Config::from_file("config.yaml".as_ref())?;
/// let (_shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
/// run_proxy(config, None, shutdown_rx).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_proxy(
    config: Config,
    registry: Option<Registry>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(config);
    let handler = ConnectionHandler::new(config.clone(), registry.as_ref());

    // Connection limit enforcement with semaphore
    let max_connections = config.max_connections.unwrap_or(10000);
    let connection_semaphore = Arc::new(Semaphore::new(max_connections));

    // Track active connections for graceful shutdown
    let active_connections = Arc::new(AtomicUsize::new(0));
    let mut connection_handles = Vec::new();

    info!("Connection limit set to {}", max_connections);

    let mut listeners: Vec<TcpListener> = Vec::new();
    for addr_str in &config.listen_addrs {
        let addr: SocketAddr = addr_str.parse()?;
        info!("Starting listener on {}", addr);
        listeners.push(TcpListener::bind(addr).await?);
    }

    // UDP listeners for HTTP/3 and QUIC (if configured)
    let mut udp_tasks = Vec::new();
    if let Some(ref udp_addrs) = config.udp_listen_addrs {
        let udp_handler = UdpConnectionHandler::new((*config).clone(), registry.as_ref());

        for addr_str in udp_addrs {
            let addr: SocketAddr = addr_str.parse()?;
            info!("Starting UDP listener on {}", addr);

            let socket = UdpSocket::bind(addr).await?;
            let handler = udp_handler.clone();

            let udp_task = tokio::spawn(async move {
                if let Err(e) = handler.run(socket).await {
                    error!("UDP handler error on {}: {}", addr, e);
                }
            });

            udp_tasks.push(udp_task);
        }

        info!("Started {} UDP listener(s) for QUIC/HTTP3", udp_addrs.len());
    }

    info!("Proxy started, waiting for connections...");

    loop {
        let mut accepts = FuturesUnordered::new();
        for listener in &listeners {
            accepts.push(listener.accept());
        }

        tokio::select! {
            // Graceful shutdown signal from broadcast channel
            _ = shutdown_rx.recv() => {
                info!("Received shutdown signal from coordinator");
                break;
            }
            // Handle Ctrl+C
            _ = signal::ctrl_c() => {
                info!("Received Ctrl+C, initiating graceful shutdown");
                break;
            }
            // Accept new connections
            Some(result) = accepts.next() => {
                match result {
                    Ok((socket, addr)) => {
                        // Try to acquire connection permit
                        match connection_semaphore.clone().try_acquire_owned() {
                            Ok(permit) => {
                                let handler = handler.clone();
                                let active = active_connections.clone();

                                // Increment active connection counter
                                active.fetch_add(1, Ordering::Relaxed);

                                // Spawn connection handler task
                                let handle = tokio::spawn(async move {
                                    handler.handle_connection(socket, addr).await;

                                    // Decrement counter and release permit when done
                                    active.fetch_sub(1, Ordering::Relaxed);
                                    drop(permit);
                                });

                                connection_handles.push(handle);

                                // Cleanup completed handles to prevent unbounded growth
                                connection_handles.retain(|h| !h.is_finished());
                            }
                            Err(_) => {
                                warn!(
                                    "Connection limit ({}) reached, rejecting connection from {}",
                                    max_connections, addr
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                    }
                }
            }
        }
    }

    // Graceful shutdown: wait for active connections to complete
    let active_count = active_connections.load(Ordering::Relaxed);
    info!(
        "Shutting down proxy, waiting for {} active TCP connections to complete",
        active_count
    );

    let shutdown_timeout_secs = config.shutdown_timeout.unwrap_or(30);
    let shutdown_timeout_duration = Duration::from_secs(shutdown_timeout_secs);

    // Wait for all TCP connection tasks to complete with timeout
    let tcp_shutdown_result = timeout(shutdown_timeout_duration, async {
        for handle in connection_handles {
            let _ = handle.await;
        }
    })
    .await;

    match tcp_shutdown_result {
        Ok(_) => {
            info!("All TCP connections completed gracefully");
        }
        Err(_) => {
            let remaining = active_connections.load(Ordering::Relaxed);
            warn!(
                "TCP shutdown timeout ({} seconds) reached, {} connections may be incomplete",
                shutdown_timeout_secs, remaining
            );
        }
    }

    // Abort UDP tasks (they run indefinitely until stopped)
    if !udp_tasks.is_empty() {
        info!("Stopping {} UDP listener(s)", udp_tasks.len());
        for task in udp_tasks {
            task.abort();
        }
    }

    info!("Proxy shutdown complete");
    Ok(())
}

const TLS_HANDSHAKE: u8 = 0x16;
const TLS_VERSION_MAJOR: u8 = 0x03;
const CLIENT_HELLO: u8 = 0x01;
const SNI_EXTENSION: u16 = 0x0000;
const ALPN_EXTENSION: u16 = 0x0010;

/// Errors that can occur during SNI extraction from TLS ClientHello.
#[derive(Debug)]
pub enum SniError {
    /// The TLS version major byte is not 0x03 (TLS 1.0-1.3)
    InvalidTlsVersion,
    /// The TLS record type is not 0x16 (Handshake)
    InvalidHandshakeType,
    /// The handshake message is not ClientHello (0x01)
    InvalidClientHello,
    /// The SNI extension is missing or malformed
    InvalidSniFormat,
    /// The TLS record or handshake message is incomplete
    MessageTruncated,
}

impl std::fmt::Display for SniError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SniError::InvalidTlsVersion => write!(f, "Invalid TLS version"),
            SniError::InvalidHandshakeType => write!(f, "Invalid handshake type"),
            SniError::InvalidClientHello => write!(f, "Invalid Client Hello"),
            SniError::InvalidSniFormat => write!(f, "Invalid SNI format"),
            SniError::MessageTruncated => write!(f, "Message truncated"),
        }
    }
}

impl std::error::Error for SniError {}

/// Extracts the Server Name Indication (SNI) from a TLS ClientHello record.
///
/// This function parses a TLS 1.0-1.3 ClientHello handshake message and extracts
/// the hostname from the SNI extension. The implementation uses zero-copy parsing
/// for optimal performance.
///
/// # Arguments
///
/// * `record` - A byte slice containing the complete TLS record including headers
///
/// # Returns
///
/// * `Ok(String)` - The extracted hostname
/// * `Err(SniError)` - If the record is invalid, truncated, or doesn't contain SNI
///
/// # Errors
///
/// This function will return an error if:
/// - The TLS record is truncated or too short
/// - The TLS version is invalid (not 0x03 major version)
/// - The handshake message is not a ClientHello (0x01)
/// - The SNI extension is missing or malformed
/// - The SNI hostname is not valid UTF-8
///
/// # Examples
///
/// ```
/// use sniproxy_core::extract_sni;
///
/// // Build a simple TLS ClientHello with SNI
/// let mut record = vec![
///     0x16, 0x03, 0x01, 0x00, 0x30,  // TLS Record
///     0x01, 0x00, 0x00, 0x2C,        // ClientHello
///     0x03, 0x03,                    // Version
/// ];
/// record.extend_from_slice(&[0; 32]);  // Random
/// record.extend_from_slice(&[
///     0x00,                          // Session ID
///     0x00, 0x02, 0x00, 0x00,       // Cipher suites
///     0x01, 0x00,                    // Compression
///     0x00, 0x10,                    // Extensions length
///     0x00, 0x00,                    // SNI type
///     0x00, 0x0C,                    // SNI length
///     0x00, 0x0A,                    // SNI list length
///     0x00,                          // hostname type
///     0x00, 0x07,                    // name length
///     // "example" in ASCII
///     0x65, 0x78, 0x61, 0x6D, 0x70, 0x6C, 0x65,
/// ]);
///
/// assert_eq!(extract_sni(&record).unwrap(), "example");
/// ```
pub fn extract_sni(record: &[u8]) -> Result<String, SniError> {
    tracing::debug!(
        "Starting SNI extraction from TLS record of length {}",
        record.len()
    );

    // Minimum length checks
    if record.len() < 5 {
        tracing::debug!("Record too short for TLS header");
        return Err(SniError::MessageTruncated);
    }

    // Verify TLS record header
    if record[0] != TLS_HANDSHAKE {
        tracing::debug!("Not a TLS handshake record: {:02x}", record[0]);
        return Err(SniError::InvalidHandshakeType);
    }

    if record[1] != TLS_VERSION_MAJOR {
        tracing::debug!("Invalid TLS version major: {:02x}", record[1]);
        return Err(SniError::InvalidTlsVersion);
    }

    // Get record length and validate
    let record_length = ((record[3] as usize) << 8) | (record[4] as usize);
    tracing::debug!("TLS record length from header: {}", record_length);

    if record.len() < record_length + 5 {
        tracing::debug!(
            "Record truncated. Expected: {}, Got: {}",
            record_length + 5,
            record.len()
        );
        return Err(SniError::MessageTruncated);
    }

    // Start parsing handshake message
    let handshake_start = 5;
    if record.len() < handshake_start + 4 {
        tracing::debug!("Record too short for handshake header");
        return Err(SniError::MessageTruncated);
    }

    // Verify Client Hello
    if record[handshake_start] != CLIENT_HELLO {
        tracing::debug!(
            "Not a Client Hello message: {:02x}",
            record[handshake_start]
        );
        return Err(SniError::InvalidClientHello);
    }

    // Get handshake length
    let handshake_length = ((record[handshake_start + 1] as usize) << 16)
        | ((record[handshake_start + 2] as usize) << 8)
        | (record[handshake_start + 3] as usize);
    tracing::debug!("Handshake length: {}", handshake_length);

    if record.len() < handshake_start + 4 + handshake_length {
        tracing::debug!("Handshake message truncated");
        return Err(SniError::MessageTruncated);
    }

    // Skip over version and random
    let mut pos = handshake_start + 4 + 2 + 32;

    // Skip session ID
    if record.len() < pos + 1 {
        return Err(SniError::MessageTruncated);
    }
    let session_id_length = record[pos] as usize;
    pos += 1 + session_id_length;

    // Skip cipher suites
    if record.len() < pos + 2 {
        return Err(SniError::MessageTruncated);
    }
    let cipher_suites_length = ((record[pos] as usize) << 8) | (record[pos + 1] as usize);
    pos += 2 + cipher_suites_length;

    // Skip compression methods
    if record.len() < pos + 1 {
        return Err(SniError::MessageTruncated);
    }
    let compression_methods_length = record[pos] as usize;
    pos += 1 + compression_methods_length;

    // Extensions length
    if record.len() < pos + 2 {
        return Err(SniError::MessageTruncated);
    }
    let extensions_length = ((record[pos] as usize) << 8) | (record[pos + 1] as usize);
    pos += 2;

    if record.len() < pos + extensions_length {
        tracing::debug!(
            "Extensions truncated. Expected length: {}, Remaining: {}",
            extensions_length,
            record.len() - pos
        );
        return Err(SniError::MessageTruncated);
    }

    let extensions_end = pos + extensions_length;
    while pos + 4 <= extensions_end {
        let extension_type = ((record[pos] as u16) << 8) | (record[pos + 1] as u16);
        let extension_length = ((record[pos + 2] as usize) << 8) | (record[pos + 3] as usize);
        pos += 4;

        if extension_type == SNI_EXTENSION {
            if pos + extension_length > extensions_end {
                return Err(SniError::MessageTruncated);
            }

            // Parse SNI extension
            if extension_length < 2 {
                return Err(SniError::InvalidSniFormat);
            }

            let sni_list_length = ((record[pos] as usize) << 8) | (record[pos + 1] as usize);
            pos += 2;

            if sni_list_length + 2 > extension_length {
                return Err(SniError::InvalidSniFormat);
            }

            while pos + 3 <= extensions_end {
                let name_type = record[pos];
                let name_length = ((record[pos + 1] as usize) << 8) | (record[pos + 2] as usize);
                pos += 3;

                if pos + name_length > extensions_end {
                    return Err(SniError::MessageTruncated);
                }

                if name_type == 0 {
                    // host_name
                    return match std::str::from_utf8(&record[pos..pos + name_length]) {
                        Ok(s) => Ok(s.to_string()),
                        Err(_) => Err(SniError::InvalidSniFormat),
                    };
                }

                pos += name_length;
            }
        } else {
            pos += extension_length;
        }
    }

    Err(SniError::InvalidSniFormat)
}

/// Extracts the Application-Layer Protocol Negotiation (ALPN) from a TLS ClientHello record.
///
/// This function parses the TLS ClientHello ALPN extension and returns the first
/// protocol in the list. Common protocols include "h2" (HTTP/2), "h3" (HTTP/3),
/// and "http/1.1".
///
/// # Arguments
///
/// * `record` - A byte slice containing the complete TLS record
///
/// # Returns
///
/// * `Some(&str)` - The first ALPN protocol name if found
/// * `None` - If the record is invalid, truncated, or doesn't contain ALPN
///
/// # Examples
///
/// ```no_run
/// use sniproxy_core::extract_alpn;
///
/// // In real usage, you would receive a TLS ClientHello from a client connection
/// // This is a simplified example showing the API
/// let client_hello: Vec<u8> = vec![]; // Would come from actual TLS handshake
///
/// // Extract ALPN protocol if present
/// if let Some(protocol) = extract_alpn(&client_hello) {
///     match protocol {
///         "h2" => println!("Client supports HTTP/2"),
///         "h3" => println!("Client supports HTTP/3"),
///         "http/1.1" => println!("Client supports HTTP/1.1"),
///         _ => println!("Unknown protocol: {}", protocol),
///     }
/// }
/// ```
pub fn extract_alpn(record: &[u8]) -> Option<&str> {
    // Skip the record header (5 bytes) and go to the handshake message
    let handshake_start = 5;

    // Ensure we have enough bytes for basic validation
    if record.len() < handshake_start + 4 {
        return None;
    }

    // Validate this is a ClientHello
    if record[0] != TLS_HANDSHAKE || record[handshake_start] != CLIENT_HELLO {
        return None;
    }

    // Get handshake length
    let handshake_length = ((record[handshake_start + 1] as usize) << 16)
        | ((record[handshake_start + 2] as usize) << 8)
        | (record[handshake_start + 3] as usize);

    if record.len() < handshake_start + 4 + handshake_length {
        return None;
    }

    // Skip over version and random
    let mut pos = handshake_start + 4 + 2 + 32;

    // Skip session ID
    if record.len() < pos + 1 {
        return None;
    }
    let session_id_length = record[pos] as usize;
    pos += 1 + session_id_length;

    // Skip cipher suites
    if record.len() < pos + 2 {
        return None;
    }
    let cipher_suites_length = ((record[pos] as usize) << 8) | (record[pos + 1] as usize);
    pos += 2 + cipher_suites_length;

    // Skip compression methods
    if record.len() < pos + 1 {
        return None;
    }
    let compression_methods_length = record[pos] as usize;
    pos += 1 + compression_methods_length;

    // Extensions length
    if record.len() < pos + 2 {
        return None;
    }
    let extensions_length = ((record[pos] as usize) << 8) | (record[pos + 1] as usize);
    pos += 2;

    if record.len() < pos + extensions_length {
        return None;
    }

    let extensions_end = pos + extensions_length;
    while pos + 4 <= extensions_end {
        let extension_type = ((record[pos] as u16) << 8) | (record[pos + 1] as u16);
        let extension_length = ((record[pos + 2] as usize) << 8) | (record[pos + 3] as usize);
        pos += 4;

        if extension_type == ALPN_EXTENSION {
            if pos + extension_length > extensions_end || extension_length < 2 {
                return None;
            }

            // ALPN extension data format:
            // - 2 bytes: ALPN list length
            // - For each protocol:
            //   - 1 byte: protocol name length
            //   - N bytes: protocol name

            let alpn_list_length = ((record[pos] as usize) << 8) | (record[pos + 1] as usize);
            pos += 2;

            if alpn_list_length + 2 > extension_length || pos + alpn_list_length > extensions_end {
                return None;
            }

            // We'll just extract the first protocol for now
            if pos < extensions_end {
                let protocol_length = record[pos] as usize;
                pos += 1;

                if pos + protocol_length <= extensions_end {
                    // Return the first protocol as a string
                    if let Ok(protocol) = std::str::from_utf8(&record[pos..pos + protocol_length]) {
                        return Some(protocol);
                    }
                }
            }

            return None;
        }

        pos += extension_length;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sni_simple() {
        // A simplified but valid TLS ClientHello with SNI extension
        let mut record = vec![
            // TLS Record
            0x16, 0x03, 0x01, 0x00, 0x30, // Type, Version, Length
            // Handshake
            0x01, 0x00, 0x00, 0x2C, // Type (ClientHello), Length
            0x03, 0x03, // Version
        ];
        record.extend_from_slice(&[0; 32]); // Random
        record.extend_from_slice(&[
            0x00, // Session ID length
            0x00, 0x02, // Cipher suites length
            0x00, 0x00, // Cipher suites
            0x01, 0x00, // Compression methods
            0x00, 0x10, // Extensions length
            // SNI extension
            0x00, 0x00, // Type (SNI)
            0x00, 0x0C, // Length
            0x00, 0x0A, // SNI list length
            0x00, // SNI type (hostname)
            0x00, 0x07, // SNI length
            // Test domain name
            0x65, 0x78, 0x61, 0x6D, 0x70, 0x6C, 0x65,
        ]);

        assert_eq!(extract_sni(&record).unwrap(), "example");
    }

    #[test]
    fn test_extract_sni_longer_domain() {
        let domain = "subdomain.example.com";
        let domain_bytes = domain.as_bytes();
        let domain_len = domain_bytes.len() as u16;

        // Calculate lengths
        let sni_list_len = 3 + domain_len; // type(1) + length(2) + domain
        let sni_ext_len = 2 + sni_list_len; // list_length(2) + list
        let extensions_len = 4 + sni_ext_len; // type(2) + length(2) + data
        let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len; // version + random + session_id + ciphers + compression + extensions_header + extensions
        let record_len = 4 + handshake_len; // handshake_header + handshake_data

        let mut record = vec![
            0x16,
            0x03,
            0x01,
            (record_len >> 8) as u8,
            (record_len & 0xff) as u8, // TLS Record length
            0x01,                      // ClientHello
            ((handshake_len as u32) >> 16) as u8,
            (handshake_len >> 8) as u8,
            (handshake_len & 0xff) as u8,
            0x03,
            0x03, // Version
        ];
        record.extend_from_slice(&[0; 32]); // Random
        record.extend_from_slice(&[
            0x00, // Session ID length
            0x00,
            0x02, // Cipher suites length
            0x00,
            0x00, // Cipher suites
            0x01,
            0x00, // Compression methods
            (extensions_len >> 8) as u8,
            (extensions_len & 0xff) as u8, // Extensions length
            // SNI extension
            0x00,
            0x00, // Type (SNI)
            (sni_ext_len >> 8) as u8,
            (sni_ext_len & 0xff) as u8,
            (sni_list_len >> 8) as u8,
            (sni_list_len & 0xff) as u8,
            0x00, // SNI type (hostname)
            (domain_len >> 8) as u8,
            (domain_len & 0xff) as u8,
        ]);
        record.extend_from_slice(domain_bytes);

        assert_eq!(extract_sni(&record).unwrap(), domain);
    }

    #[test]
    fn test_extract_sni_truncated_record() {
        let record = vec![0x16, 0x03, 0x01]; // Too short
        match extract_sni(&record) {
            Err(SniError::MessageTruncated) => {}
            _ => panic!("Expected MessageTruncated error"),
        }
    }

    #[test]
    fn test_extract_sni_invalid_handshake_type() {
        let record = vec![
            0x15, 0x03, 0x01, 0x00, 0x02, // Not a handshake (0x15 is alert)
            0x01, 0x00,
        ];
        match extract_sni(&record) {
            Err(SniError::InvalidHandshakeType) => {}
            _ => panic!("Expected InvalidHandshakeType error"),
        }
    }

    #[test]
    fn test_extract_sni_invalid_tls_version() {
        let record = vec![
            0x16, 0x02, 0x01, 0x00, 0x05, // Invalid TLS version (0x02 instead of 0x03)
            0x01, 0x00, 0x00, 0x00,
        ];
        match extract_sni(&record) {
            Err(SniError::InvalidTlsVersion) => {}
            _ => panic!("Expected InvalidTlsVersion error"),
        }
    }

    #[test]
    fn test_extract_sni_not_client_hello() {
        // Build a proper-length record with ServerHello (0x02) instead of ClientHello (0x01)
        let handshake_data = {
            let mut data = vec![0x03, 0x03]; // Version
            data.extend_from_slice(&[0; 32]); // Random
            data.extend(&[
                0x00, // Session ID length
                0x00, 0x02, // Cipher suites length
                0x00, 0x00, // Cipher suites
                0x01, 0x00, // Compression methods
                0x00, 0x00, // Extensions length
            ]);
            data
        };

        let handshake_len = handshake_data.len();
        let record_len = 4 + handshake_len; // handshake header (4 bytes) + data

        let mut record = vec![
            0x16,
            0x03,
            0x01, // TLS record header
            (record_len >> 8) as u8,
            (record_len & 0xff) as u8,
            0x02, // ServerHello (not ClientHello!)
            ((handshake_len as u32) >> 16) as u8,
            (handshake_len >> 8) as u8,
            (handshake_len & 0xff) as u8,
        ];
        record.extend_from_slice(&handshake_data);

        match extract_sni(&record) {
            Err(SniError::InvalidClientHello) => {}
            other => panic!("Expected InvalidClientHello error, got: {:?}", other),
        }
    }

    #[test]
    fn test_extract_sni_no_sni_extension() {
        let mut record = vec![
            0x16, 0x03, 0x01, 0x00, 0x30, 0x01, 0x00, 0x00, 0x2C, 0x03, 0x03,
        ];
        record.extend_from_slice(&[0; 32]);
        record.extend_from_slice(&[
            0x00, // Session ID length
            0x00, 0x02, // Cipher suites length
            0x00, 0x00, 0x01, 0x00, // Compression methods
            0x00, 0x04, // Extensions length
            // Different extension (not SNI)
            0x00, 0x17, // Type (extended_master_secret)
            0x00, 0x00, // Length 0
        ]);

        match extract_sni(&record) {
            Err(SniError::InvalidSniFormat) => {}
            _ => panic!("Expected InvalidSniFormat error"),
        }
    }

    #[test]
    fn test_extract_alpn_http2() {
        let protocol = b"h2";
        let protocol_len = protocol.len() as u8;
        let alpn_list_len = 1 + protocol_len as u16; // length_byte + protocol
        let alpn_ext_len = 2 + alpn_list_len; // list_length(2) + list
        let extensions_len = 4 + alpn_ext_len; // type(2) + length(2) + data
        let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len; // version + random + session_id + ciphers + compression + extensions_header + extensions
        let record_len = 4 + handshake_len; // handshake_header + handshake_data

        let mut record = vec![
            0x16,
            0x03,
            0x01,
            (record_len >> 8) as u8,
            (record_len & 0xff) as u8,
            0x01, // ClientHello
            ((handshake_len as u32) >> 16) as u8,
            (handshake_len >> 8) as u8,
            (handshake_len & 0xff) as u8,
            0x03,
            0x03, // Version
        ];
        record.extend_from_slice(&[0; 32]); // Random
        record.extend_from_slice(&[
            0x00, // Session ID length
            0x00,
            0x02, // Cipher suites length
            0x00,
            0x00, // Cipher suites
            0x01,
            0x00, // Compression methods
            (extensions_len >> 8) as u8,
            (extensions_len & 0xff) as u8, // Extensions length
            // ALPN extension
            0x00,
            0x10, // Type (ALPN = 0x0010)
            (alpn_ext_len >> 8) as u8,
            (alpn_ext_len & 0xff) as u8,
            (alpn_list_len >> 8) as u8,
            (alpn_list_len & 0xff) as u8,
            protocol_len, // Protocol length
        ]);
        record.extend_from_slice(protocol);

        assert_eq!(extract_alpn(&record), Some("h2"));
    }

    #[test]
    fn test_extract_alpn_http3() {
        let protocol = b"h3";
        let protocol_len = protocol.len() as u8;
        let alpn_list_len = 1 + protocol_len as u16;
        let alpn_ext_len = 2 + alpn_list_len;
        let extensions_len = 4 + alpn_ext_len;
        let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len;
        let record_len = 4 + handshake_len;

        let mut record = vec![
            0x16,
            0x03,
            0x01,
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
            // ALPN extension
            0x00,
            0x10,
            (alpn_ext_len >> 8) as u8,
            (alpn_ext_len & 0xff) as u8,
            (alpn_list_len >> 8) as u8,
            (alpn_list_len & 0xff) as u8,
            protocol_len,
        ]);
        record.extend_from_slice(protocol);

        assert_eq!(extract_alpn(&record), Some("h3"));
    }

    #[test]
    fn test_extract_alpn_no_alpn() {
        let mut record = vec![
            0x16, 0x03, 0x01, 0x00, 0x30, 0x01, 0x00, 0x00, 0x2C, 0x03, 0x03,
        ];
        record.extend_from_slice(&[0; 32]);
        record.extend_from_slice(&[
            0x00, 0x00, 0x02, 0x00, 0x00, 0x01, 0x00, 0x00, 0x04, // Different extension
            0x00, 0x00, // SNI
            0x00, 0x00,
        ]);

        assert_eq!(extract_alpn(&record), None);
    }

    #[test]
    fn test_extract_alpn_truncated() {
        let record = vec![0x16, 0x03, 0x01];
        assert_eq!(extract_alpn(&record), None);
    }

    #[test]
    fn test_sni_error_display() {
        assert_eq!(
            SniError::InvalidTlsVersion.to_string(),
            "Invalid TLS version"
        );
        assert_eq!(
            SniError::InvalidHandshakeType.to_string(),
            "Invalid handshake type"
        );
        assert_eq!(
            SniError::InvalidClientHello.to_string(),
            "Invalid Client Hello"
        );
        assert_eq!(SniError::InvalidSniFormat.to_string(), "Invalid SNI format");
        assert_eq!(SniError::MessageTruncated.to_string(), "Message truncated");
    }
}
