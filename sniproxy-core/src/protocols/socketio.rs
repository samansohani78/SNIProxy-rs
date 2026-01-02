//! Socket.IO protocol detection and handling
//!
//! Supports Engine.IO v3 and v4 with polling and WebSocket transports

use std::error::Error;

/// Detect Socket.IO from HTTP request
///
/// Checks for Socket.IO-specific patterns in the HTTP request:
/// - `/socket.io/` path prefix
/// - `EIO=` query parameter (Engine.IO version)
///
/// # Arguments
///
/// * `request` - The HTTP request string (including headers)
///
/// # Returns
///
/// Returns `true` if the request appears to be a Socket.IO request
///
/// # Examples
///
/// ```
/// use sniproxy_core::protocols::socketio::detect_socketio;
///
/// let req = "GET /socket.io/?EIO=4&transport=polling HTTP/1.1\r\n";
/// assert!(detect_socketio(req));
/// ```
pub fn detect_socketio(request: &str) -> bool {
    // Check for /socket.io/ path
    if request.contains("/socket.io/") {
        return true;
    }

    // Check for EIO query parameter (Engine.IO version)
    if request.contains("EIO=3") || request.contains("EIO=4") {
        return true;
    }

    false
}

/// Extract Socket.IO namespace from request path
///
/// Parses the request to extract the Socket.IO namespace.
/// Returns the default namespace ("/") if none is specified.
///
/// # Arguments
///
/// * `path` - The request path with query parameters
///
/// # Returns
///
/// Returns the namespace string or "/" if not specified
pub fn extract_namespace(path: &str) -> Result<String, Box<dyn Error>> {
    // Parse: /socket.io/?EIO=4&transport=polling&namespace=/admin
    for param in path.split('&') {
        if let Some(ns) = param.strip_prefix("namespace=") {
            return Ok(ns.to_string());
        }
    }

    Ok("/".to_string()) // Default namespace
}

/// Socket.IO transport type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transport {
    /// Long-polling transport
    Polling,
    /// WebSocket transport
    WebSocket,
    /// Unknown transport
    Unknown,
}

/// Detect transport type from request
///
/// Determines whether the Socket.IO connection is using
/// polling or WebSocket transport.
///
/// # Arguments
///
/// * `request` - The HTTP request string
///
/// # Returns
///
/// Returns the detected transport type
pub fn detect_transport(request: &str) -> Transport {
    if request.contains("transport=polling") {
        Transport::Polling
    } else if request.contains("transport=websocket") {
        Transport::WebSocket
    } else {
        Transport::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socketio_detection() {
        assert!(detect_socketio(
            "GET /socket.io/?EIO=4&transport=polling HTTP/1.1"
        ));
        assert!(detect_socketio("GET /socket.io/?EIO=3 HTTP/1.1"));
        assert!(!detect_socketio("GET /api/data HTTP/1.1"));
    }

    #[test]
    fn test_transport_detection() {
        let req = "GET /socket.io/?EIO=4&transport=polling HTTP/1.1";
        assert_eq!(detect_transport(req), Transport::Polling);

        let req_ws = "GET /socket.io/?EIO=4&transport=websocket HTTP/1.1";
        assert_eq!(detect_transport(req_ws), Transport::WebSocket);
    }

    #[test]
    fn test_namespace_extraction() {
        let path = "/socket.io/?EIO=4&transport=polling&namespace=/admin";
        assert_eq!(extract_namespace(path).unwrap(), "/admin");

        let default_path = "/socket.io/?EIO=4&transport=polling";
        assert_eq!(extract_namespace(default_path).unwrap(), "/");
    }
}
