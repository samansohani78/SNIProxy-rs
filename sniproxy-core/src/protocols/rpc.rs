//! Generic RPC over HTTP detection
//!
//! Detects generic RPC frameworks that use HTTP as transport

/// Detect generic RPC from request path or headers
///
/// Checks for common RPC path patterns:
/// - `/rpc`
/// - `/api/rpc`
/// - `/jsonrpc`
/// - `/xmlrpc`
/// - Paths containing "rpc"
///
/// # Arguments
///
/// * `request` - The HTTP request string (including path and headers)
///
/// # Returns
///
/// Returns `true` if the request appears to be a generic RPC request
///
/// # Examples
///
/// ```
/// use sniproxy_core::protocols::rpc::detect_rpc;
///
/// let req = "POST /api/rpc HTTP/1.1\r\n";
/// assert!(detect_rpc(req));
/// ```
pub fn detect_rpc(request: &str) -> bool {
    let lower = request.to_lowercase();

    // Check for common RPC path patterns
    if lower.contains(" /rpc ") || lower.contains(" /rpc?") || lower.contains(" /rpc/") {
        return true;
    }

    if lower.contains(" /api/rpc") {
        return true;
    }

    if lower.contains("/jsonrpc") || lower.contains("/xmlrpc") {
        return true;
    }

    // Check for RPC in path (but not if it's part of a longer word)
    if let Some(path_start) = lower.find(" /")
        && let Some(path_end) = lower[path_start..].find(" http/")
    {
        let path = &lower[path_start..path_start + path_end];
        if path.contains("/rpc") {
            return true;
        }
    }

    false
}

/// Extract RPC method from request path
///
/// Attempts to extract the RPC method name from the request path.
/// Common patterns:
/// - `/rpc/methodName`
/// - `/api/rpc/methodName`
///
/// # Arguments
///
/// * `request` - The HTTP request string
///
/// # Returns
///
/// Returns the method name if found
pub fn extract_rpc_method(request: &str) -> Option<String> {
    // Extract the path from the request
    let parts: Vec<&str> = request.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let path = parts[1];

    // Try to extract method after /rpc/ or /api/rpc/
    if let Some(pos) = path.find("/rpc/") {
        let method_part = &path[pos + 5..]; // Skip "/rpc/"
        if let Some(end) = method_part.find('?') {
            return Some(method_part[..end].to_string());
        } else if let Some(end) = method_part.find('#') {
            return Some(method_part[..end].to_string());
        } else if !method_part.is_empty() {
            return Some(method_part.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_detection() {
        assert!(detect_rpc("POST /rpc HTTP/1.1"));
        assert!(detect_rpc("POST /api/rpc HTTP/1.1"));
        assert!(detect_rpc("POST /rpc/method HTTP/1.1"));
        assert!(detect_rpc("GET /jsonrpc HTTP/1.1"));
        assert!(detect_rpc("POST /xmlrpc HTTP/1.1"));
    }

    #[test]
    fn test_not_rpc() {
        assert!(!detect_rpc("GET /api/users HTTP/1.1"));
        assert!(!detect_rpc("POST /graphql HTTP/1.1"));
        assert!(!detect_rpc("GET / HTTP/1.1"));
    }

    #[test]
    fn test_method_extraction() {
        let req = "POST /rpc/getUserInfo HTTP/1.1";
        assert_eq!(extract_rpc_method(req), Some("getUserInfo".to_string()));

        let req_with_params = "POST /rpc/sendMessage?id=123 HTTP/1.1";
        assert_eq!(
            extract_rpc_method(req_with_params),
            Some("sendMessage".to_string())
        );

        let req_no_method = "POST /rpc HTTP/1.1";
        assert_eq!(extract_rpc_method(req_no_method), None);
    }

    #[test]
    fn test_case_insensitive() {
        assert!(detect_rpc("POST /RPC HTTP/1.1"));
        assert!(detect_rpc("POST /API/RPC HTTP/1.1"));
    }
}
