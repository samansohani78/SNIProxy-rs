//! JSON-RPC 1.0 and 2.0 protocol support

use serde_json::Value;

/// Detect JSON-RPC from request body
///
/// Checks if the request body contains valid JSON-RPC 1.0 or 2.0 content.
///
/// JSON-RPC 2.0 requirements:
/// - Must have `"jsonrpc": "2.0"` field
/// - Must have `"method"` field
///
/// JSON-RPC 1.0 requirements:
/// - Must have `"method"` field
///
/// Also supports batch requests (arrays of JSON-RPC requests).
///
/// # Arguments
///
/// * `body` - The HTTP request body as bytes
///
/// # Returns
///
/// Returns `true` if the body appears to be a JSON-RPC request
///
/// # Examples
///
/// ```
/// use sniproxy_core::protocols::jsonrpc::detect_jsonrpc;
///
/// let body = br#"{"jsonrpc":"2.0","method":"test","params":[],"id":1}"#;
/// assert!(detect_jsonrpc(body));
/// ```
pub fn detect_jsonrpc(body: &[u8]) -> bool {
    if let Ok(json) = serde_json::from_slice::<Value>(body) {
        // JSON-RPC 2.0: Must have "jsonrpc": "2.0"
        if json.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0") {
            return true;
        }

        // JSON-RPC 1.0: Must have "method" field
        if json.get("method").is_some() {
            return true;
        }

        // Batch requests (array)
        if json.is_array()
            && let Some(arr) = json.as_array()
        {
            return arr
                .iter()
                .any(|v| v.get("jsonrpc").is_some() || v.get("method").is_some());
        }
    }

    false
}

/// Validate JSON-RPC batch size
///
/// Ensures that batch requests don't exceed a maximum size limit.
///
/// # Arguments
///
/// * `body` - The HTTP request body as bytes
/// * `max_size` - Maximum number of requests allowed in a batch
///
/// # Returns
///
/// Returns `Ok(())` if the batch is valid, or an error message if too large
pub fn validate_batch(body: &[u8], max_size: usize) -> Result<(), String> {
    if let Ok(json) = serde_json::from_slice::<Value>(body)
        && let Some(arr) = json.as_array()
        && arr.len() > max_size
    {
        return Err(format!(
            "Batch size {} exceeds limit {}",
            arr.len(),
            max_size
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonrpc_v2_detection() {
        let body = br#"{"jsonrpc":"2.0","method":"test","params":[],"id":1}"#;
        assert!(detect_jsonrpc(body));
    }

    #[test]
    fn test_jsonrpc_v1_detection() {
        let body = br#"{"method":"test","params":[],"id":1}"#;
        assert!(detect_jsonrpc(body));
    }

    #[test]
    fn test_jsonrpc_batch() {
        let body = br#"[{"jsonrpc":"2.0","method":"test1","id":1},{"jsonrpc":"2.0","method":"test2","id":2}]"#;
        assert!(detect_jsonrpc(body));
        assert!(validate_batch(body, 10).is_ok());
        assert!(validate_batch(body, 1).is_err());
    }

    #[test]
    fn test_not_jsonrpc() {
        let body = br#"{"data":"value"}"#;
        assert!(!detect_jsonrpc(body));
    }
}
