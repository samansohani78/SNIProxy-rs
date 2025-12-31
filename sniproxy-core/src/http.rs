use prometheus::IntCounter;
use std::error::Error;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};

// Performance tuning constants
const READ_BUFFER_SIZE: usize = 16384; // 16KB for better throughput
const COPY_BUFFER_SIZE: usize = 32768; // 32KB for bidirectional copy

// Constants for HTTP protocol detection
const WEBSOCKET_UPGRADE: &str = "websocket";
const SWITCHING_PROTOCOLS: &[u8] = b"HTTP/1.1 101";
const CONTENT_TYPE_HEADER: &str = "content-type:";
const GRPC_CONTENT_TYPE: &str = "application/grpc";

// HTTP/2 frame type constants
const HTTP2_FRAME_TYPE_HEADERS: u8 = 0x1;

#[derive(Debug)]
#[allow(dead_code)] // Some variants reserved for future protocol detection features
pub enum HttpError {
    Io(io::Error),
    NoHostHeader,
    InvalidRequest,
    WebSocketUpgradeFailed,
    Http2FrameError,
    GrpcDetectionFailed,
    Timeout,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Io(e) => write!(f, "IO error: {}", e),
            HttpError::NoHostHeader => write!(f, "No Host header found"),
            HttpError::InvalidRequest => write!(f, "Invalid HTTP request"),
            HttpError::WebSocketUpgradeFailed => write!(f, "WebSocket upgrade failed"),
            HttpError::Http2FrameError => write!(f, "HTTP/2 frame parsing error"),
            HttpError::GrpcDetectionFailed => write!(f, "gRPC detection failed"),
            HttpError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl Error for HttpError {}

impl From<io::Error> for HttpError {
    fn from(err: io::Error) -> Self {
        HttpError::Io(err)
    }
}

impl From<tokio::time::error::Elapsed> for HttpError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        HttpError::Timeout
    }
}

#[inline]
pub async fn extract_host(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
) -> Result<(String, usize), HttpError> {
    let mut total_read = 0;
    loop {
        let mut chunk = [0; READ_BUFFER_SIZE];
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

        if total_read > READ_BUFFER_SIZE * 2 {
            // Limit headers to prevent abuse
            return Err(HttpError::InvalidRequest);
        }
    }
}

/// Tunnels an HTTP connection with metrics tracking
pub async fn tunnel_http(
    client: &mut TcpStream,
    initial_data: &[u8],
    host: &str,
    port: u16,
    metrics: Option<(IntCounter, IntCounter)>,
) -> Result<(), HttpError> {
    let addr = format!("{}:{}", host, port);
    let mut server = TcpStream::connect(addr).await?;

    // Forward the initial request
    server.write_all(initial_data).await?;

    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut server_read, mut server_write) = tokio::io::split(&mut server);

    // If metrics are enabled, use the tracking copy, otherwise use the standard copy
    if let Some((tx_counter, rx_counter)) = metrics {
        tokio::try_join!(
            copy_with_metrics(&mut client_read, &mut server_write, tx_counter),
            copy_with_metrics(&mut server_read, &mut client_write, rx_counter)
        )?;
    } else {
        tokio::try_join!(
            tokio::io::copy(&mut client_read, &mut server_write),
            tokio::io::copy(&mut server_read, &mut client_write)
        )?;
    }

    Ok(())
}

/// Tunnels a WebSocket connection with upgrade detection
pub async fn tunnel_websocket(
    client: &mut TcpStream,
    initial_data: &[u8],
    host: &str,
    port: u16,
    metrics: Option<(IntCounter, IntCounter)>,
) -> Result<(), HttpError> {
    let addr = format!("{}:{}", host, port);
    let mut server = TcpStream::connect(addr).await?;

    // Forward the initial request
    server.write_all(initial_data).await?;

    // We need to inspect the response to detect WebSocket upgrade
    // First, we'll read the response headers from the server
    let mut response_buffer = [0u8; 4096]; // Enough for typical headers
    let mut response_len = 0;
    let mut _is_websocket = false; // Prefixed with underscore as it's used for debugging
    let mut headers_complete = false;

    // Read with timeout to prevent hanging
    let response_timeout = Duration::from_secs(10);

    while response_len < response_buffer.len() && !headers_complete {
        let n = timeout(
            response_timeout,
            server.read(&mut response_buffer[response_len..]),
        )
        .await??;

        if n == 0 {
            break; // End of stream
        }

        response_len += n;

        // Check if we've read the end of headers (double CRLF)
        if response_len >= 4 {
            for i in 3..response_len {
                if response_buffer[i - 3..=i] == [b'\r', b'\n', b'\r', b'\n'] {
                    headers_complete = true;
                    break;
                }
            }
        }

        // Detect WebSocket upgrade (101 Switching Protocols)
        if response_len >= SWITCHING_PROTOCOLS.len() {
            let response_str =
                std::str::from_utf8(&response_buffer[..response_len]).unwrap_or_default();

            if response_str
                .starts_with(std::str::from_utf8(SWITCHING_PROTOCOLS).unwrap_or_default())
                && response_str
                    .to_lowercase()
                    .contains(&format!("upgrade: {}", WEBSOCKET_UPGRADE))
            {
                _is_websocket = true;
                println!("WebSocket upgrade detected");
            }
        }
    }

    // Send the response back to the client
    client.write_all(&response_buffer[..response_len]).await?;

    // Now continue with standard bidirectional copy
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut server_read, mut server_write) = tokio::io::split(&mut server);

    // If metrics are enabled, use the tracking copy, otherwise use the standard copy
    if let Some((tx_counter, rx_counter)) = metrics {
        tokio::try_join!(
            copy_with_metrics(&mut client_read, &mut server_write, tx_counter),
            copy_with_metrics(&mut server_read, &mut client_write, rx_counter)
        )?;
    } else {
        tokio::try_join!(
            tokio::io::copy(&mut client_read, &mut server_write),
            tokio::io::copy(&mut server_read, &mut client_write)
        )?;
    }

    Ok(())
}

/// Parses HTTP/2 frames to detect gRPC traffic
// TODO: Integrate gRPC detection into connection handler
// This function is currently unused but kept for future implementation
#[allow(dead_code)]
pub async fn detect_grpc(stream: &mut TcpStream) -> Result<bool, HttpError> {
    // Buffer for reading HTTP/2 frame header (9 bytes)
    let mut frame_header = [0u8; 9];
    let mut is_grpc = false;
    let detection_timeout = Duration::from_secs(5);

    // Read the HTTP/2 frame header
    timeout(detection_timeout, stream.read_exact(&mut frame_header)).await??;

    // Parse HTTP/2 frame header
    let frame_length = ((frame_header[0] as usize) << 16)
        | ((frame_header[1] as usize) << 8)
        | (frame_header[2] as usize);
    let frame_type = frame_header[3];

    // If it's a HEADERS frame, we need to check for gRPC content-type
    if frame_type == HTTP2_FRAME_TYPE_HEADERS && frame_length > 0 {
        // Read the frame payload (limiting to a reasonable size to prevent abuse)
        let max_payload = frame_length.min(4096);
        let mut payload = vec![0u8; max_payload];
        timeout(detection_timeout, stream.read_exact(&mut payload)).await??;

        // Convert to string (lossy) for header inspection
        let payload_str = String::from_utf8_lossy(&payload);

        // Check for gRPC content-type
        if payload_str.to_lowercase().contains(CONTENT_TYPE_HEADER)
            && payload_str.to_lowercase().contains(GRPC_CONTENT_TYPE)
        {
            is_grpc = true;
        }
    }

    Ok(is_grpc)
}

/// Extracts :authority pseudo-header from HTTP/2 HEADERS frame
///
/// This function reads the HTTP/2 HEADERS frame and attempts to extract
/// the :authority pseudo-header which contains the target hostname.
///
/// # Arguments
///
/// * `stream` - The TCP stream to read from
///
/// # Returns
///
/// Returns a tuple of (authority, frame_data) where frame_data contains
/// the frame header and payload that was read, so it can be forwarded.
///
/// # Note
///
/// This is a simplified HTTP/2 frame parser. It searches for the :authority
/// field in the HPACK-encoded headers using pattern matching rather than
/// a full HPACK decoder. This works for most common cases.
pub async fn extract_http2_authority(
    stream: &mut TcpStream,
) -> Result<(String, Vec<u8>), HttpError> {
    let detection_timeout = Duration::from_secs(5);

    // Read HTTP/2 frame header (9 bytes)
    let mut frame_header = [0u8; 9];
    timeout(detection_timeout, stream.read_exact(&mut frame_header)).await??;

    // Parse frame header
    let frame_length = ((frame_header[0] as usize) << 16)
        | ((frame_header[1] as usize) << 8)
        | (frame_header[2] as usize);
    let frame_type = frame_header[3];

    // Verify it's a HEADERS frame (type 0x1)
    if frame_type != HTTP2_FRAME_TYPE_HEADERS {
        return Err(HttpError::Http2FrameError);
    }

    // Sanity check frame length (prevent abuse)
    if frame_length == 0 || frame_length > 16384 {
        return Err(HttpError::Http2FrameError);
    }

    // Read the frame payload
    let mut payload = vec![0u8; frame_length];
    timeout(detection_timeout, stream.read_exact(&mut payload)).await??;

    // Combine frame header and payload for forwarding
    let mut frame_data = Vec::with_capacity(9 + frame_length);
    frame_data.extend_from_slice(&frame_header);
    frame_data.extend_from_slice(&payload);

    // Search for :authority in the payload
    // In HPACK encoding, :authority is a static table entry (index 1)
    // or can be a literal header field

    // Try to find literal ":authority" string in payload
    if let Some(pos) = payload.windows(10).position(|w| w == b":authority") {
        // Found :authority field, now extract the value
        // The value typically follows after the field name
        let value_start = pos + 10;

        if value_start < payload.len() {
            // In HPACK, strings are length-prefixed
            // For simplicity, we'll look for the next few bytes as the length

            // Try to find a reasonable hostname pattern after :authority
            // Look for printable ASCII characters that form a hostname
            let remaining = &payload[value_start..];

            // Skip potential padding/flags bytes and find the actual value
            for offset in 0..remaining.len().min(10) {
                if let Some(authority) = extract_authority_value(&remaining[offset..]) {
                    return Ok((authority, frame_data));
                }
            }
        }
    }

    // Alternative: Look for indexed :authority (static table index 1)
    // HPACK uses variable-length integers, index 1 could be encoded as 0x01 or 0x81
    for i in 0..payload.len().saturating_sub(20) {
        if payload[i] == 0x01 || payload[i] == 0x81 || payload[i] == 0x41 {
            // Might be indexed :authority, check if followed by a hostname pattern
            if let Some(authority) = extract_authority_value(&payload[i + 1..])
                && (authority.contains('.') || authority.contains(':'))
            {
                return Ok((authority, frame_data));
            }
        }
    }

    Err(HttpError::Http2FrameError)
}

/// Helper function to extract authority value from HPACK-encoded data
fn extract_authority_value(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    // Check if first byte is a length indicator
    let len = data[0] as usize;

    // Sanity check: hostname should be between 3 and 255 characters
    if !(3..=255).contains(&len) || len + 1 > data.len() {
        return None;
    }

    // Extract the hostname
    if let Ok(hostname) = std::str::from_utf8(&data[1..=len]) {
        // Validate it looks like a hostname (contains at least one dot or colon for port)
        // and only contains valid hostname characters
        if is_valid_hostname(hostname) {
            return Some(hostname.to_string());
        }
    }

    None
}

/// Validates if a string is a valid hostname
#[inline]
fn is_valid_hostname(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }

    // Check for valid hostname characters
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':' || c == '_')
        && (s.contains('.') || s.contains(':'))
}

/// Copy data with metrics tracking
#[inline]
async fn copy_with_metrics<R, W>(
    reader: &mut R,
    writer: &mut W,
    counter: IntCounter,
) -> Result<u64, io::Error>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut buffer = [0u8; COPY_BUFFER_SIZE];
    let mut total = 0u64;

    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n]).await?;

        // Update the counter with the bytes transferred
        counter.inc_by(n as u64);
        total += n as u64;
    }

    Ok(total)
}

#[inline]
fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    // Optimized search for \r\n\r\n using windows iterator
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

#[inline]
fn extract_host_header(headers: &[u8]) -> Option<String> {
    let headers_str = std::str::from_utf8(headers).ok()?;
    for line in headers_str.lines() {
        // Case-insensitive comparison without allocating lowercase string
        if line.len() > 5 && line[..5].eq_ignore_ascii_case("host:") {
            return Some(line[5..].trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_headers_end_simple() {
        let buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert_eq!(find_headers_end(buffer), Some(buffer.len()));
    }

    #[test]
    fn test_find_headers_end_with_body() {
        let buffer = b"POST / HTTP/1.1\r\nHost: example.com\r\n\r\nBody data here";
        let headers_end = find_headers_end(buffer).unwrap();
        assert_eq!(
            &buffer[..headers_end],
            b"POST / HTTP/1.1\r\nHost: example.com\r\n\r\n"
        );
    }

    #[test]
    fn test_find_headers_end_no_end() {
        let buffer = b"GET / HTTP/1.1\r\nHost: example.com\r\n";
        assert_eq!(find_headers_end(buffer), None);
    }

    #[test]
    fn test_find_headers_end_too_short() {
        let buffer = b"GET";
        assert_eq!(find_headers_end(buffer), None);
    }

    #[test]
    fn test_extract_host_header_simple() {
        let headers = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert_eq!(
            extract_host_header(headers),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_extract_host_header_with_port() {
        let headers = b"GET / HTTP/1.1\r\nHost: example.com:8080\r\n\r\n";
        assert_eq!(
            extract_host_header(headers),
            Some("example.com:8080".to_string())
        );
    }

    #[test]
    fn test_extract_host_header_with_whitespace() {
        let headers = b"GET / HTTP/1.1\r\nHost:   example.com   \r\n\r\n";
        assert_eq!(
            extract_host_header(headers),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_extract_host_header_case_insensitive() {
        let headers = b"GET / HTTP/1.1\r\nHOST: example.com\r\n\r\n";
        assert_eq!(
            extract_host_header(headers),
            Some("example.com".to_string())
        );

        let headers2 = b"GET / HTTP/1.1\r\nhOsT: example.com\r\n\r\n";
        assert_eq!(
            extract_host_header(headers2),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_extract_host_header_missing() {
        let headers = b"GET / HTTP/1.1\r\nUser-Agent: Test\r\n\r\n";
        assert_eq!(extract_host_header(headers), None);
    }

    #[test]
    fn test_extract_host_header_multiple_headers() {
        let headers =
            b"GET / HTTP/1.1\r\nUser-Agent: Test\r\nHost: example.com\r\nAccept: */*\r\n\r\n";
        assert_eq!(
            extract_host_header(headers),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_extract_host_header_invalid_utf8() {
        let headers = b"GET / HTTP/1.1\r\nHost: \xFF\xFE\r\n\r\n";
        assert_eq!(extract_host_header(headers), None);
    }

    #[test]
    fn test_http_error_display() {
        assert_eq!(HttpError::NoHostHeader.to_string(), "No Host header found");
        assert_eq!(
            HttpError::InvalidRequest.to_string(),
            "Invalid HTTP request"
        );
        assert_eq!(HttpError::Timeout.to_string(), "Operation timed out");
        assert_eq!(
            HttpError::WebSocketUpgradeFailed.to_string(),
            "WebSocket upgrade failed"
        );
        assert_eq!(
            HttpError::Http2FrameError.to_string(),
            "HTTP/2 frame parsing error"
        );
        assert_eq!(
            HttpError::GrpcDetectionFailed.to_string(),
            "gRPC detection failed"
        );
    }

    #[test]
    fn test_http_error_from_io() {
        let io_error = io::Error::new(io::ErrorKind::ConnectionRefused, "test");
        let http_error: HttpError = io_error.into();
        assert!(matches!(http_error, HttpError::Io(_)));
    }
}
