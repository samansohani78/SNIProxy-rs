//! UDP connection handling for QUIC/HTTP3
//!
//! This module provides UDP datagram handling for QUIC-based protocols including HTTP/3.
//! It manages:
//! - UDP session tracking with automatic cleanup
//! - QUIC protocol detection
//! - Bidirectional datagram forwarding between client and backend
//! - Session expiration and resource management
//!
//! # Architecture
//!
//! ```text
//! Client UDP → SNIProxy UDP Handler → Backend UDP
//!     ↓              ↓                      ↓
//!  QUIC         Session Tracking        QUIC Server
//! Initial       (DashMap-based)
//! Packet
//! ```
//!
//! # Example
//!
//! ```no_run
//! use sniproxy_core::udp_connection::UdpConnectionHandler;
//! use sniproxy_config::Config;
//! use tokio::net::UdpSocket;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = Config::parse(r#"
//! listen_addrs: ["0.0.0.0:443"]
//! timeouts: { connect: 10, client_hello: 10, idle: 300 }
//! metrics: { enabled: false, address: "127.0.0.1:9000" }
//! "#)?;
//!
//! let socket = UdpSocket::bind("0.0.0.0:443").await?;
//! let handler = UdpConnectionHandler::new(config, None);
//! handler.run(socket).await?;
//! # Ok(())
//! # }
//! ```

use dashmap::DashMap;
use prometheus::Registry;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

use crate::Config;

/// Maximum UDP datagram size (MTU-safe)
const MAX_DATAGRAM_SIZE: usize = 1350;

/// Default session timeout in seconds
const SESSION_TIMEOUT_SECS: u64 = 30;

/// Maximum number of concurrent UDP sessions
const MAX_SESSIONS: usize = 10_000;

/// UDP connection handler managing QUIC/HTTP3 sessions
#[derive(Clone)]
pub struct UdpConnectionHandler {
    #[allow(dead_code)]
    config: Arc<Config>,
    sessions: Arc<DashMap<SocketAddr, UdpSession>>,
    #[allow(dead_code)]
    metrics: Option<Arc<UdpMetrics>>,
}

/// UDP session state
struct UdpSession {
    backend_socket: Arc<UdpSocket>,
    backend_addr: SocketAddr,
    last_activity: Instant,
    #[allow(dead_code)]
    protocol: UdpProtocol,
    #[allow(dead_code)]
    bytes_tx: u64,
    #[allow(dead_code)]
    bytes_rx: u64,
}

/// UDP protocol type
#[derive(Debug, Clone, Copy, PartialEq)]
enum UdpProtocol {
    Quic,
    Unknown,
}

/// UDP metrics (placeholder for future implementation)
struct UdpMetrics {
    #[allow(dead_code)]
    registry: Registry,
}

impl UdpConnectionHandler {
    /// Creates a new UDP connection handler
    ///
    /// # Arguments
    ///
    /// * `config` - Proxy configuration
    /// * `registry` - Optional Prometheus registry for metrics
    ///
    /// # Example
    ///
    /// ```
    /// use sniproxy_core::udp_connection::UdpConnectionHandler;
    /// use sniproxy_config::Config;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = Config::parse(r#"
    /// listen_addrs: ["0.0.0.0:443"]
    /// timeouts: { connect: 10, client_hello: 10, idle: 300 }
    /// metrics: { enabled: false, address: "127.0.0.1:9000" }
    /// "#)?;
    ///
    /// let handler = UdpConnectionHandler::new(config, None);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(config: Config, registry: Option<&Registry>) -> Self {
        Self {
            config: Arc::new(config),
            sessions: Arc::new(DashMap::new()),
            metrics: registry.map(|r| {
                Arc::new(UdpMetrics {
                    registry: r.clone(),
                })
            }),
        }
    }

    /// Main UDP handling loop
    ///
    /// Receives datagrams from clients, manages sessions, and forwards traffic to backends.
    ///
    /// # Arguments
    ///
    /// * `socket` - UDP socket to receive datagrams from
    ///
    /// # Errors
    ///
    /// Returns an error if socket operations fail or sessions cannot be created.
    pub async fn run(&self, socket: UdpSocket) -> Result<(), Box<dyn std::error::Error>> {
        let socket = Arc::new(socket);
        let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];

        info!("UDP handler started");

        loop {
            // Receive datagram from client
            let (len, src_addr) = match socket.recv_from(&mut buf).await {
                Ok(result) => result,
                Err(e) => {
                    error!("Failed to receive UDP datagram: {}", e);
                    continue;
                }
            };

            let data = &buf[..len];

            // Detect protocol
            let protocol = match self.detect_protocol(data) {
                Ok(p) => p,
                Err(e) => {
                    debug!("Protocol detection failed: {}", e);
                    continue;
                }
            };

            // Handle packet based on protocol
            match protocol {
                UdpProtocol::Quic => {
                    if let Err(e) = self.handle_quic_packet(data, src_addr, &socket).await {
                        warn!("Failed to handle QUIC packet from {}: {}", src_addr, e);
                    }
                }
                UdpProtocol::Unknown => {
                    debug!("Unknown UDP protocol from {}", src_addr);
                }
            }

            // Periodic cleanup
            if self.sessions.len().is_multiple_of(100) {
                self.cleanup_sessions();
            }
        }
    }

    /// Detects protocol from UDP datagram
    #[inline]
    fn detect_protocol(&self, data: &[u8]) -> Result<UdpProtocol, Box<dyn std::error::Error>> {
        if data.is_empty() {
            return Ok(UdpProtocol::Unknown);
        }

        // QUIC: Long header has bit 7 set (0x80)
        // QUIC packets start with a header byte where:
        // - Long header: bit 7 = 1 (initial, 0-RTT, handshake, retry)
        // - Short header: bit 7 = 0 (1-RTT packets)
        if data.len() >= 5 && (data[0] & 0x80) != 0 {
            return Ok(UdpProtocol::Quic);
        }

        Ok(UdpProtocol::Unknown)
    }

    /// Handles QUIC packet forwarding
    async fn handle_quic_packet(
        &self,
        data: &[u8],
        src_addr: SocketAddr,
        client_socket: &Arc<UdpSocket>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get or create session
        let session_created = !self.sessions.contains_key(&src_addr);

        if session_created {
            self.create_session(src_addr, data, client_socket).await?;
        }

        // Forward packet to backend
        if let Some(mut session) = self.sessions.get_mut(&src_addr) {
            session
                .backend_socket
                .send_to(data, session.backend_addr)
                .await?;
            session.bytes_tx += data.len() as u64;
            session.last_activity = Instant::now();
            debug!(
                "Forwarded {} bytes from {} to backend {}",
                data.len(),
                src_addr,
                session.backend_addr
            );
        }

        Ok(())
    }

    /// Creates a new UDP session
    async fn create_session(
        &self,
        src_addr: SocketAddr,
        initial_packet: &[u8],
        client_socket: &Arc<UdpSocket>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Enforce session limit
        if self.sessions.len() >= MAX_SESSIONS {
            return Err("Max UDP sessions reached".into());
        }

        // Extract SNI from QUIC Initial packet
        let sni = extract_quic_sni(initial_packet)?;
        debug!("Extracted SNI from QUIC: {}", sni);

        // Resolve backend address
        let backend_addr = self.resolve_backend(&sni).await?;

        // Create backend socket
        let backend_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);

        let session = UdpSession {
            backend_socket: Arc::clone(&backend_socket),
            backend_addr,
            last_activity: Instant::now(),
            protocol: UdpProtocol::Quic,
            bytes_tx: 0,
            bytes_rx: 0,
        };

        self.sessions.insert(src_addr, session);

        // Spawn response handler
        self.spawn_response_handler(src_addr, backend_socket, Arc::clone(client_socket))
            .await;

        info!("Created UDP session for {} → {}", src_addr, backend_addr);

        Ok(())
    }

    /// Resolves backend address from SNI
    async fn resolve_backend(&self, sni: &str) -> Result<SocketAddr, Box<dyn std::error::Error>> {
        // Use default HTTPS port for QUIC/HTTP3
        let port = 443;
        let addr_str = format!("{}:{}", sni, port);

        let addr = tokio::net::lookup_host(&addr_str)
            .await?
            .next()
            .ok_or_else(|| format!("Failed to resolve {}", addr_str))?;

        Ok(addr)
    }

    /// Spawns background task to handle responses from backend
    async fn spawn_response_handler(
        &self,
        client_addr: SocketAddr,
        backend_socket: Arc<UdpSocket>,
        client_socket: Arc<UdpSocket>,
    ) {
        let sessions = Arc::clone(&self.sessions);

        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM_SIZE];
            let timeout_duration = Duration::from_secs(SESSION_TIMEOUT_SECS);

            loop {
                match tokio::time::timeout(timeout_duration, backend_socket.recv(&mut buf)).await {
                    Ok(Ok(len)) => {
                        // Forward response to client
                        if let Err(e) = client_socket.send_to(&buf[..len], client_addr).await {
                            error!("Failed to send to client {}: {}", client_addr, e);
                            break;
                        }

                        // Update session stats
                        if let Some(mut session) = sessions.get_mut(&client_addr) {
                            session.bytes_rx += len as u64;
                            session.last_activity = Instant::now();
                        }

                        debug!("Forwarded {} bytes from backend to {}", len, client_addr);
                    }
                    Ok(Err(e)) => {
                        error!("Backend recv error: {}", e);
                        break;
                    }
                    Err(_) => {
                        // Timeout - session expired
                        debug!("UDP session timeout for {}", client_addr);
                        break;
                    }
                }
            }

            // Remove session on exit
            sessions.remove(&client_addr);
            info!("Closed UDP session for {}", client_addr);
        });
    }

    /// Cleans up expired sessions
    fn cleanup_sessions(&self) {
        let now = Instant::now();
        let timeout = Duration::from_secs(SESSION_TIMEOUT_SECS);

        let expired_count = self.sessions.len();
        self.sessions
            .retain(|_, session| now.duration_since(session.last_activity) < timeout);

        let remaining = self.sessions.len();
        if expired_count > remaining {
            debug!(
                "Cleaned up {} expired UDP sessions",
                expired_count - remaining
            );
        }
    }
}

/// Extracts SNI from QUIC Initial packet
///
/// # Arguments
///
/// * `packet` - Raw QUIC packet data
///
/// # Returns
///
/// Returns the extracted SNI hostname or an error if parsing fails.
///
/// # Implementation
///
/// QUIC Initial packets have the following structure:
/// ```text
/// +--------+--------+--------+--------+--------+
/// | Header | DCID   | SCID   | Token  | Payload|
/// |  Form  | Len    | Len    | Len    |        |
/// +--------+--------+--------+--------+--------+
/// ```
///
/// The payload contains CRYPTO frames with TLS ClientHello.
/// We search for the TLS handshake (0x16) byte and attempt SNI extraction.
pub fn extract_quic_sni(packet: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    // Minimum QUIC Initial packet size check
    if packet.len() < 20 {
        return Err("Packet too small to be QUIC Initial".into());
    }

    // Verify this is a QUIC long header (bit 7 = 1)
    if (packet[0] & 0x80) == 0 {
        return Err("Not a QUIC long header packet".into());
    }

    // Parse QUIC long header to find payload
    // Byte 0: Header form and flags
    // Bytes 1-4: Version
    // Byte 5: DCID Length
    if packet.len() < 6 {
        return Err("Packet truncated at DCID length".into());
    }

    let dcid_len = packet[5] as usize;
    let mut offset = 6 + dcid_len;

    // Skip DCID
    if packet.len() < offset + 1 {
        return Err("Packet truncated at SCID length".into());
    }

    // SCID Length
    let scid_len = packet[offset] as usize;
    offset += 1 + scid_len;

    // Skip Token Length (VarInt)
    if packet.len() < offset + 1 {
        return Err("Packet truncated at token".into());
    }

    let token_len = packet[offset] as usize;
    offset += 1 + token_len;

    // Skip Length field (VarInt encoding, simplified)
    if packet.len() < offset + 2 {
        return Err("Packet truncated at length".into());
    }
    offset += 2;

    // Now we're at the payload, which contains CRYPTO frames with TLS ClientHello
    // Search for TLS ClientHello (0x16 = Handshake)
    let payload = &packet[offset..];

    // Try to find TLS record in payload
    // Look for 0x16 (TLS Handshake) byte
    for i in 0..payload.len().saturating_sub(5) {
        if payload[i] == 0x16 {
            // Found potential TLS handshake
            // Try to extract SNI from this position
            if let Ok(sni) = crate::extract_sni(&payload[i..]) {
                return Ok(sni);
            }
        }
    }

    Err("No valid SNI found in QUIC packet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_detection_quic_long_header() {
        let config = create_test_config();
        let handler = UdpConnectionHandler::new(config, None);

        // QUIC long header (bit 7 set)
        let quic_packet = vec![0xC0, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(
            handler.detect_protocol(&quic_packet).unwrap(),
            UdpProtocol::Quic
        );
    }

    #[test]
    fn test_protocol_detection_non_quic() {
        let config = create_test_config();
        let handler = UdpConnectionHandler::new(config, None);

        // Not a QUIC packet (bit 7 not set)
        let non_quic_packet = vec![0x40, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(
            handler.detect_protocol(&non_quic_packet).unwrap(),
            UdpProtocol::Unknown
        );
    }

    #[test]
    fn test_protocol_detection_empty() {
        let config = create_test_config();
        let handler = UdpConnectionHandler::new(config, None);

        let empty_packet = vec![];
        assert_eq!(
            handler.detect_protocol(&empty_packet).unwrap(),
            UdpProtocol::Unknown
        );
    }

    #[test]
    fn test_session_cleanup() {
        let config = create_test_config();
        let handler = UdpConnectionHandler::new(config, None);

        // Cleanup should not crash on empty sessions
        handler.cleanup_sessions();
        assert_eq!(handler.sessions.len(), 0);
    }

    #[test]
    fn test_quic_sni_extraction_too_small() {
        let small_packet = vec![0xC0; 10];
        let result = extract_quic_sni(&small_packet);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too small"));
    }

    #[test]
    fn test_quic_sni_extraction_not_long_header() {
        // Short header packet (bit 7 = 0)
        let short_header = vec![0x40; 50];
        let result = extract_quic_sni(&short_header);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Not a QUIC long header")
        );
    }

    #[test]
    fn test_quic_sni_extraction_truncated() {
        // Long header but truncated
        let truncated = vec![0xC0, 0x00, 0x00, 0x00, 0x01];
        let result = extract_quic_sni(&truncated);
        assert!(result.is_err());
    }

    #[test]
    fn test_quic_sni_extraction_no_sni() {
        // Valid QUIC structure but no SNI
        let mut packet = vec![
            0xC0, // Long header
            0x00, 0x00, 0x00, 0x01, // Version
            0x08, // DCID Length = 8
        ];
        packet.extend_from_slice(&[0; 8]); // DCID
        packet.push(0x00); // SCID Length = 0
        packet.push(0x00); // Token Length = 0
        packet.extend_from_slice(&[0x00, 0x10]); // Length field
        packet.extend_from_slice(&[0; 50]); // Payload without TLS

        let result = extract_quic_sni(&packet);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No valid SNI"));
    }

    fn create_test_config() -> Config {
        Config::parse(
            r#"
listen_addrs: ["0.0.0.0:443"]
timeouts:
  connect: 10
  client_hello: 10
  idle: 300
metrics:
  enabled: false
  address: "127.0.0.1:9000"
"#,
        )
        .unwrap()
    }
}
