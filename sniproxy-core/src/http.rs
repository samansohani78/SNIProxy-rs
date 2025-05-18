use std::io;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::error::Error;

#[derive(Debug)]
pub enum HttpError {
    Io(io::Error),
    NoHostHeader,
    InvalidRequest,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Io(e) => write!(f, "IO error: {}", e),
            HttpError::NoHostHeader => write!(f, "No Host header found"),
            HttpError::InvalidRequest => write!(f, "Invalid HTTP request"),
        }
    }
}

impl Error for HttpError {}

impl From<io::Error> for HttpError {
    fn from(err: io::Error) -> Self {
        HttpError::Io(err)
    }
}

pub async fn extract_host(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Result<(String, usize), HttpError> {
    let mut total_read = 0;
    loop {
        let mut chunk = [0; 8192];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(HttpError::InvalidRequest);
        }

        buffer.extend_from_slice(&chunk[..n]);
        total_read += n;

        if let Some(headers_end) = find_headers_end(buffer) {
            if let Some(host) = extract_host_header(&buffer[..headers_end]) {
                return Ok((host, total_read));
            }
            return Err(HttpError::NoHostHeader);
        }

        if total_read > 16384 {  // Limit headers to 16KB
            return Err(HttpError::InvalidRequest);
        }
    }
}

pub async fn tunnel_http(client: &mut TcpStream, initial_data: &[u8], host: &str) -> Result<(), HttpError> {
    let addr = format!("{}:80", host);
    let mut server = TcpStream::connect(addr).await?;
    
    // Forward the initial request
    server.write_all(initial_data).await?;
    
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut server_read, mut server_write) = tokio::io::split(&mut server);

    tokio::try_join!(
        tokio::io::copy(&mut client_read, &mut server_write),
        tokio::io::copy(&mut server_read, &mut client_write)
    )?;

    Ok(())
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    for i in 3..buffer.len() {
        if &buffer[i-3..=i] == b"\r\n\r\n" {
            return Some(i + 1);
        }
    }
    None
}

fn extract_host_header(headers: &[u8]) -> Option<String> {
    let headers_str = std::str::from_utf8(headers).ok()?;
    for line in headers_str.lines() {
        if line.to_lowercase().starts_with("host:") {
            return Some(line[5..].trim().to_string());
        }
    }
    None
}
