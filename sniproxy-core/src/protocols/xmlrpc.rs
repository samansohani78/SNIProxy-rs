//! XML-RPC protocol support

use roxmltree::Document;

/// Detect XML-RPC from request body
///
/// Checks if the request body contains a valid XML-RPC `<methodCall>` structure.
///
/// # Arguments
///
/// * `body` - The HTTP request body as bytes
///
/// # Returns
///
/// Returns `true` if the body appears to be an XML-RPC request
///
/// # Examples
///
/// ```
/// use sniproxy_core::protocols::xmlrpc::detect_xmlrpc;
///
/// let body = br#"<?xml version="1.0"?>
/// <methodCall>
///   <methodName>examples.getStateName</methodName>
/// </methodCall>"#;
/// assert!(detect_xmlrpc(body));
/// ```
pub fn detect_xmlrpc(body: &[u8]) -> bool {
    if let Ok(text) = std::str::from_utf8(body)
        && let Ok(doc) = Document::parse(text)
    {
        // Check for <methodCall> root element
        if doc.root_element().tag_name().name() == "methodCall" {
            return true;
        }
    }
    false
}

/// Extract method name from XML-RPC request
///
/// Parses the XML-RPC body to extract the `<methodName>` element.
///
/// # Arguments
///
/// * `body` - The HTTP request body as bytes
///
/// # Returns
///
/// Returns the method name or an error if not found
pub fn extract_method(body: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let text = std::str::from_utf8(body)?;
    let doc = Document::parse(text)?;

    for node in doc.descendants() {
        if node.tag_name().name() == "methodName" && let Some(text) = node.text() {
            return Ok(text.to_string());
        }
    }

    Err("No methodName found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xmlrpc_detection() {
        let body = br#"<?xml version="1.0"?>
<methodCall>
  <methodName>examples.getStateName</methodName>
  <params>
    <param>
      <value><i4>40</i4></value>
    </param>
  </params>
</methodCall>"#;

        assert!(detect_xmlrpc(body));
        assert_eq!(extract_method(body).unwrap(), "examples.getStateName");
    }

    #[test]
    fn test_not_xmlrpc() {
        let body = br#"<?xml version="1.0"?><data><value>test</value></data>"#;
        assert!(!detect_xmlrpc(body));
    }

    #[test]
    fn test_invalid_xml() {
        let body = b"not xml at all";
        assert!(!detect_xmlrpc(body));
    }
}
