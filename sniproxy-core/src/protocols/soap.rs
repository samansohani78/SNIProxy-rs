//! SOAP 1.1 and 1.2 protocol support

use quick_xml::Reader;
use quick_xml::events::Event;

/// Detect SOAP from headers or body
///
/// Checks for SOAP indicators in either the HTTP headers or request body:
/// - SOAPAction header (SOAP 1.1)
/// - SOAP Envelope in the XML body (SOAP 1.1/1.2)
///
/// # Arguments
///
/// * `headers` - The HTTP headers as a string
/// * `body` - The HTTP request body as bytes
///
/// # Returns
///
/// Returns `true` if the request appears to be a SOAP request
///
/// # Examples
///
/// ```
/// use sniproxy_core::protocols::soap::detect_soap;
///
/// let headers = "POST /StockQuote HTTP/1.1\r\nSOAPAction: \"http://example.com/GetPrice\"\r\n";
/// let body = b"";
/// assert!(detect_soap(headers, body));
/// ```
pub fn detect_soap(headers: &str, body: &[u8]) -> bool {
    // Check SOAPAction header (SOAP 1.1)
    if headers.to_lowercase().contains("soapaction:") {
        return true;
    }

    // Check for SOAP envelope in body
    detect_soap_envelope(body)
}

/// Detect SOAP envelope structure in XML body
///
/// Looks for the SOAP Envelope element in the XML.
///
/// # Arguments
///
/// * `body` - The HTTP request body as bytes
///
/// # Returns
///
/// Returns `true` if a SOAP Envelope is found
fn detect_soap_envelope(body: &[u8]) -> bool {
    let mut reader = Reader::from_reader(body);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = name.local_name();
                // Check for soap:Envelope or soap12:Envelope
                if local.as_ref() == b"Envelope" {
                    return true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return false,
            _ => {}
        }
        buf.clear();
    }

    false
}

/// Extract SOAPAction from header
///
/// Parses the SOAPAction header value from the HTTP headers.
///
/// # Arguments
///
/// * `headers` - The HTTP headers as a string
///
/// # Returns
///
/// Returns the SOAPAction value if found
pub fn extract_soap_action(headers: &str) -> Option<String> {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("soapaction:") {
            // Split only on the first colon to preserve URLs like "http://..."
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                return Some(parts[1].trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soap_envelope_detection() {
        let body = br#"<?xml version="1.0"?>
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/">
  <soap:Body>
    <GetPrice xmlns="http://www.example.com/stock">
      <StockName>IBM</StockName>
    </GetPrice>
  </soap:Body>
</soap:Envelope>"#;

        assert!(detect_soap_envelope(body));
    }

    #[test]
    fn test_soap_action_header() {
        let headers = "POST /StockQuote HTTP/1.1\r\n\
                      Content-Type: text/xml\r\n\
                      SOAPAction: \"http://www.example.com/GetStockPrice\"\r\n";

        assert!(detect_soap(headers, b""));
        assert_eq!(
            extract_soap_action(headers),
            Some("http://www.example.com/GetStockPrice".to_string())
        );
    }

    #[test]
    fn test_not_soap() {
        let body = br#"<?xml version="1.0"?><data><value>test</value></data>"#;
        let headers = "POST /api HTTP/1.1\r\n";
        assert!(!detect_soap(headers, body));
    }

    #[test]
    fn test_case_insensitive_header() {
        let headers = "POST /Test HTTP/1.1\r\nsoapaction: \"test\"\r\n";
        assert!(detect_soap(headers, b""));
    }
}
