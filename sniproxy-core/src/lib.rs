pub mod connection;
mod http;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};
use connection::ConnectionHandler;
use sniproxy_config::Config;
use prometheus::Registry;
use futures::stream::FuturesUnordered;
use futures::StreamExt;

pub async fn run_proxy(config: Config, registry: Option<Registry>) -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(config);
    let handler = ConnectionHandler::new(config.clone(), registry.as_ref());

    let mut listeners: Vec<TcpListener> = Vec::new();
    for addr_str in &config.listen_addrs {
        let addr: SocketAddr = addr_str.parse()?;
        info!("Starting listener on {}", addr);
        listeners.push(TcpListener::bind(addr).await?);
    }

    info!("Proxy started, waiting for connections...");

    loop {
        let mut accepts = FuturesUnordered::new();
        for listener in &listeners {
            accepts.push(listener.accept());
        }

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received shutdown signal");
                break;
            }
            Some(result) = accepts.next() => {
                match result {
                    Ok((socket, addr)) => {
                        let handler = handler.clone();
                        tokio::spawn(async move {
                            handler.handle_connection(socket, addr).await;
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                    }
                }
            }
        }
    }

    info!("Shutting down proxy");
    Ok(())
}

const TLS_HANDSHAKE: u8 = 0x16;
const TLS_VERSION_MAJOR: u8 = 0x03;
const CLIENT_HELLO: u8 = 0x01;
const SNI_EXTENSION: u16 = 0x0000;

#[derive(Debug)]
pub enum SniError {
    InvalidTlsVersion,
    InvalidHandshakeType,
    InvalidClientHello,
    InvalidSniFormat,
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

pub fn extract_sni(record: &[u8]) -> Result<String, SniError> {
    tracing::debug!("Starting SNI extraction from TLS record of length {}", record.len());
    
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
        tracing::debug!("Record truncated. Expected: {}, Got: {}", record_length + 5, record.len());
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
        tracing::debug!("Not a Client Hello message: {:02x}", record[handshake_start]);
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
        tracing::debug!("Extensions truncated. Expected length: {}, Remaining: {}", 
            extensions_length, record.len() - pos);
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

                if name_type == 0 {  // host_name
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sni_simple() {
        // A simplified but valid TLS ClientHello with SNI extension
        let mut record = vec![
            // TLS Record
            0x16, 0x03, 0x01, 0x00, 0x30,  // Type, Version, Length
            // Handshake
            0x01, 0x00, 0x00, 0x2C,        // Type (ClientHello), Length
            0x03, 0x03,                    // Version
        ];
        record.extend_from_slice(&[0; 32]); // Random
        record.extend_from_slice(&[
            0x00,                          // Session ID length
            0x00, 0x02,                    // Cipher suites length
            0x00, 0x00,                    // Cipher suites
            0x01, 0x00,                    // Compression methods
            0x00, 0x10,                    // Extensions length
            // SNI extension
            0x00, 0x00,                    // Type (SNI)
            0x00, 0x0C,                    // Length
            0x00, 0x0A,                    // SNI list length
            0x00,                          // SNI type (hostname)
            0x00, 0x07,                    // SNI length
            // Test domain name
            0x65, 0x78, 0x61, 0x6D, 0x70, 0x6C, 0x65,
        ]);

        assert_eq!(extract_sni(assert_eq!(extract_sni(&record).unwrap(), "example");record).unwrap(), "ip.me");
    }
}
