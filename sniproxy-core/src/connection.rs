use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{self, AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};
use prometheus::{IntCounterVec, IntCounter, Registry, Opts};
use crate::SniError;
use crate::http::{self, HttpError};
use sniproxy_config::Config;

const MAX_TLS_HEADER_SIZE: usize = 16384;  // Increased size for TLS header
const MIN_TLS_HEADER_SIZE: usize = 5;      // Minimum size for TLS header

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

#[derive(Debug)]
enum Protocol {
    Http,
    Https,
}

#[derive(Clone)]
pub struct ConnectionHandler {
    config: Arc<Config>,
    metrics: Option<Arc<ConnectionMetrics>>,
}

struct ConnectionMetrics {
    bytes_transferred: IntCounterVec,
}

impl ConnectionMetrics {
    fn new(registry: &Registry) -> Self {
        let bytes_transferred = IntCounterVec::new(
            Opts::new(
                "sniproxy_bytes_transferred_total",
                "Total bytes transferred per host"
            ),
            &["host", "direction"]
        ).unwrap();
        registry.register(Box::new(bytes_transferred.clone())).unwrap();
        
        Self { bytes_transferred }
    }
}

impl ConnectionHandler {
    pub fn new(config: Arc<Config>, registry: Option<&Registry>) -> Self {
        let metrics = registry.map(|r| Arc::new(ConnectionMetrics::new(r)));
        Self { config, metrics }
    }

    pub async fn handle_connection(&self, mut client: TcpStream, client_addr: SocketAddr) {
        let peer = client_addr.to_string();
        info!(peer, "New connection");

        match self.process_connection(&mut client).await {
            Ok(_) => info!(peer, "Connection completed"),
            Err(e) => error!(peer, error = %e, "Connection error"),
        }
    }

    async fn process_connection(&self, client: &mut TcpStream) -> Result<(), Box<dyn std::error::Error>> {
        // Read enough bytes to identify the protocol (largest HTTP method + 1)
        let mut peek_buf = [0u8; 8];
        let n = client.peek(&mut peek_buf).await?;
        if n == 0 {
            return Err("Empty connection".into());
        }

        // Check for HTTP methods first
        let protocol = if HTTP_METHODS.iter().any(|method| peek_buf.starts_with(method)) {
            debug!("Found HTTP method in first bytes: {:?}", String::from_utf8_lossy(&peek_buf[..n.min(4)]));
            Protocol::Http
        } else if peek_buf[0] == 0x16 {
            debug!("Found TLS handshake marker");
            Protocol::Https
        } else {
            debug!("Unknown protocol, first bytes: {:02x?}", &peek_buf[..n]);
            return Err("Unknown protocol".into());
        };

        debug!("Detected protocol: {:?}", protocol);

        match protocol {
            Protocol::Http => self.handle_http(client).await?,
            Protocol::Https => self.handle_https(client).await?,
        }

        Ok(())
    }

    async fn handle_http(&self, client: &mut TcpStream) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = Vec::with_capacity(8192);
        
        // Extract host from HTTP headers
        let (host, bytes_read) = match http::extract_host(client, &mut buffer).await {
            Ok(result) => result,
            Err(HttpError::NoHostHeader) => {
                warn!("No Host header in HTTP request");
                return Ok(());
            }
            Err(e) => return Err(Box::new(e)),
        };

        debug!(host, "Extracted Host from HTTP headers");

        // Check allowlist if configured
        if let Some(ref allowlist) = self.config.allowlist {
            if !self.is_host_allowed(&host, allowlist) {
                warn!(host, "Host not in allowlist");
                return Ok(());
            }
        }

        // Setup metrics if enabled
        let _metrics = self.metrics.as_ref().map(|m| {
            (
                m.bytes_transferred.with_label_values(&[host.as_str(), "tx"]),
                m.bytes_transferred.with_label_values(&[host.as_str(), "rx"])
            )
        });

        // Tunnel the connection
        http::tunnel_http(client, &buffer[..bytes_read], &host).await?;

        Ok(())
    }

    async fn handle_https(&self, client: &mut TcpStream) -> Result<(), Box<dyn std::error::Error>> {
        let hello_timeout = Duration::from_secs(self.config.timeouts.client_hello);
        let mut reader = BufReader::new(client);
        
        // Read and verify TLS header (5 bytes)
        let mut record = Vec::with_capacity(16384);
        record.resize(MIN_TLS_HEADER_SIZE, 0);
        
        debug!("Reading TLS header...");
        timeout(hello_timeout, reader.read_exact(&mut record[..MIN_TLS_HEADER_SIZE])).await??;

        // Verify it's a TLS handshake
        if record[0] != 0x16 {
            debug!("Not a TLS handshake, first byte: {:02x}", record[0]);
            return Err("Not a TLS handshake".into());
        }

        // Get record length and validate
        let record_length = ((record[3] as usize) << 8) | (record[4] as usize);
        debug!("TLS record length: {}", record_length);

        if record_length < 4 || record_length > MAX_TLS_HEADER_SIZE {
            debug!("Invalid TLS record length: {}", record_length);
            return Err("Invalid TLS record length".into());
        }

        // Read the rest of the record
        record.resize(MIN_TLS_HEADER_SIZE + record_length, 0);
        debug!("Reading TLS record body ({} bytes)...", record_length);
        timeout(hello_timeout, reader.read_exact(&mut record[MIN_TLS_HEADER_SIZE..])).await??;

        // Extract SNI
        debug!("Record complete, total length: {}", record.len());
        let sni = crate::extract_sni(&record)?;
        debug!(sni, "Extracted SNI from ClientHello");

        // Check allowlist if configured
        if let Some(ref allowlist) = self.config.allowlist {
            if !self.is_host_allowed(&sni, allowlist) {
                warn!(sni, "Host not in allowlist");
                return Err(Box::new(SniError::InvalidSniFormat));
            }
        }

        // Resolve and connect to target
        let target_addr = format!("{}:443", sni);
        debug!("Resolving target address: {}", target_addr);
        let addr = lookup_host(&target_addr).await?.next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Failed to resolve target"))?;

        let connect_timeout = Duration::from_secs(self.config.timeouts.connect);
        debug!("Connecting to target: {}", addr);
        let mut server = timeout(connect_timeout, TcpStream::connect(addr)).await??;

        // Setup metrics if enabled
        let metrics = self.metrics.as_ref().map(|m| {
            (
                m.bytes_transferred.with_label_values(&[sni.as_str(), "tx"]),
                m.bytes_transferred.with_label_values(&[sni.as_str(), "rx"])
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
        if allowlist.contains(&"*".to_string()) {
            return true;
        }

        let host_lower = host.to_lowercase();
        allowlist.iter().any(|pattern| {
            if pattern.starts_with("*.") {
                let suffix = &pattern[1..];
                host_lower.ends_with(suffix)
            } else {
                host_lower == pattern.to_lowercase()
            }
        })
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
            if n == 0 { break; }
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
            if n == 0 { break; }
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
