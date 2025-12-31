/// SNI Extraction Example
///
/// This example demonstrates how to extract SNI and ALPN from TLS ClientHello records.
///
/// Run with: cargo run --example sni_extraction

use sniproxy_core::{extract_sni, extract_alpn};

fn build_sample_client_hello() -> Vec<u8> {
    let domain = "www.example.com";
    let domain_bytes = domain.as_bytes();
    let domain_len = domain_bytes.len() as u16;

    let sni_list_len = 3 + domain_len;
    let sni_ext_len = 2 + sni_list_len;

    // Also add ALPN extension for h2
    let protocol = b"h2";
    let protocol_len = protocol.len() as u8;
    let alpn_list_len = 1 + protocol_len as u16;
    let alpn_ext_len = 2 + alpn_list_len;

    let extensions_len = 4 + sni_ext_len + 4 + alpn_ext_len;
    let handshake_len = 2 + 32 + 1 + 2 + 2 + 2 + 2 + extensions_len;
    let record_len = 4 + handshake_len;

    let mut record = vec![
        0x16, 0x03, 0x03,
        (record_len >> 8) as u8,
        (record_len & 0xff) as u8,
        0x01,
        ((handshake_len as u32) >> 16) as u8,
        (handshake_len >> 8) as u8,
        (handshake_len & 0xff) as u8,
        0x03, 0x03,
    ];
    record.extend_from_slice(&[0; 32]);
    record.extend_from_slice(&[
        0x00,
        0x00, 0x02,
        0x00, 0x00,
        0x01, 0x00,
        (extensions_len >> 8) as u8,
        (extensions_len & 0xff) as u8,
        // SNI extension
        0x00, 0x00,
        (sni_ext_len >> 8) as u8,
        (sni_ext_len & 0xff) as u8,
        (sni_list_len >> 8) as u8,
        (sni_list_len & 0xff) as u8,
        0x00,
        (domain_len >> 8) as u8,
        (domain_len & 0xff) as u8,
    ]);
    record.extend_from_slice(domain_bytes);

    // Add ALPN extension
    record.extend_from_slice(&[
        0x00, 0x10,
        (alpn_ext_len >> 8) as u8,
        (alpn_ext_len & 0xff) as u8,
        (alpn_list_len >> 8) as u8,
        (alpn_list_len & 0xff) as u8,
        protocol_len,
    ]);
    record.extend_from_slice(protocol);

    record
}

fn main() {
    println!("SNI and ALPN Extraction Example\n");

    // Build a sample TLS ClientHello
    let client_hello = build_sample_client_hello();
    println!("TLS ClientHello record size: {} bytes", client_hello.len());

    // Extract SNI
    match extract_sni(&client_hello) {
        Ok(hostname) => {
            println!("✓ Extracted SNI hostname: {}", hostname);
        }
        Err(e) => {
            eprintln!("✗ Failed to extract SNI: {}", e);
        }
    }

    // Extract ALPN
    match extract_alpn(&client_hello) {
        Some(protocol) => {
            println!("✓ Extracted ALPN protocol: {}", protocol);
        }
        None => {
            println!("✗ No ALPN protocol found");
        }
    }

    println!("\nExample: Testing error handling");

    // Test with truncated record
    let truncated = vec![0x16, 0x03, 0x01];
    match extract_sni(&truncated) {
        Ok(_) => println!("Unexpected success with truncated record"),
        Err(e) => println!("✓ Correctly detected error: {}", e),
    }

    // Test with invalid TLS version
    let invalid_version = vec![0x16, 0x02, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00];
    match extract_sni(&invalid_version) {
        Ok(_) => println!("Unexpected success with invalid version"),
        Err(e) => println!("✓ Correctly detected error: {}", e),
    }
}
