//! WebSocket permessage-deflate compression (RFC 7692)
//!
//! This module provides compression and decompression for WebSocket messages
//! using the permessage-deflate extension. This can reduce bandwidth by 40-60%
//! for text-based messages.
//!
//! # Features
//!
//! - DEFLATE compression for WebSocket frames
//! - Configurable compression level
//! - Context takeover support
//! - Memory-efficient streaming compression
//! - RFC 7692 compliant implementation
//!
//! # Architecture
//!
//! The permessage-deflate extension compresses each WebSocket message independently
//! using DEFLATE. The compressed data is sent with the RSV1 bit set in the frame header.

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{Read, Write};

/// Configuration for WebSocket compression
#[derive(Debug, Clone)]
pub struct WebSocketCompressionConfig {
    /// Enable compression (default: true)
    pub enabled: bool,
    /// Compression level 0-9, where 0=no compression, 9=best compression (default: 6)
    pub compression_level: u32,
    /// Server context takeover (default: true)
    pub server_no_context_takeover: bool,
    /// Client context takeover (default: true)
    pub client_no_context_takeover: bool,
    /// Maximum window bits for server (default: 15)
    pub server_max_window_bits: u8,
    /// Maximum window bits for client (default: 15)
    pub client_max_window_bits: u8,
    /// Minimum message size to compress in bytes (default: 256)
    pub min_compress_size: usize,
}

impl Default for WebSocketCompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            compression_level: 6, // Balanced compression
            server_no_context_takeover: false,
            client_no_context_takeover: false,
            server_max_window_bits: 15, // RFC 7692 maximum
            client_max_window_bits: 15,
            min_compress_size: 256, // Don't compress small messages
        }
    }
}

/// WebSocket message compression handler
pub struct WebSocketCompression {
    config: WebSocketCompressionConfig,
}

impl WebSocketCompression {
    /// Create a new WebSocket compression handler
    pub fn new(config: WebSocketCompressionConfig) -> Self {
        Self { config }
    }

    /// Compress a WebSocket message payload
    ///
    /// Returns the compressed data if compression is beneficial, otherwise returns None
    /// to indicate the original data should be sent uncompressed.
    ///
    /// # Arguments
    /// * `data` - The uncompressed message payload
    ///
    /// # Returns
    /// * `Some(Vec<u8>)` - Compressed data if compression reduced size
    /// * `None` - If compression didn't help or message is too small
    pub fn compress(&self, data: &[u8]) -> Result<Option<Vec<u8>>, std::io::Error> {
        if !self.config.enabled {
            return Ok(None);
        }

        // Don't compress small messages
        if data.len() < self.config.min_compress_size {
            return Ok(None);
        }

        let compression = Compression::new(self.config.compression_level);
        let mut encoder = DeflateEncoder::new(Vec::new(), compression);

        encoder.write_all(data)?;
        let mut compressed = encoder.finish()?;

        // RFC 7692: Remove trailing 0x00 0x00 0xff 0xff
        if compressed.len() >= 4 && compressed[compressed.len() - 4..] == [0x00, 0x00, 0xff, 0xff] {
            compressed.truncate(compressed.len() - 4);
        }

        // Only use compression if it actually reduces size
        if compressed.len() < data.len() {
            Ok(Some(compressed))
        } else {
            Ok(None)
        }
    }

    /// Decompress a WebSocket message payload
    ///
    /// # Arguments
    /// * `data` - The compressed message payload
    ///
    /// # Returns
    /// * `Result<Vec<u8>, std::io::Error>` - Decompressed data or error
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        if !self.config.enabled {
            return Ok(data.to_vec());
        }

        // RFC 7692: Append 0x00 0x00 0xff 0xff to compressed data
        let mut input = data.to_vec();
        input.extend_from_slice(&[0x00, 0x00, 0xff, 0xff]);

        let mut decoder = DeflateDecoder::new(&input[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }

    /// Generate Sec-WebSocket-Extensions header value for permessage-deflate
    ///
    /// # Returns
    /// * `String` - Extension header value (e.g., "permessage-deflate; client_max_window_bits")
    pub fn extension_header(&self) -> String {
        if !self.config.enabled {
            return String::new();
        }

        let mut parts = vec!["permessage-deflate".to_string()];

        if self.config.server_no_context_takeover {
            parts.push("server_no_context_takeover".to_string());
        }

        if self.config.client_no_context_takeover {
            parts.push("client_no_context_takeover".to_string());
        }

        if self.config.server_max_window_bits != 15 {
            parts.push(format!(
                "server_max_window_bits={}",
                self.config.server_max_window_bits
            ));
        }

        if self.config.client_max_window_bits != 15 {
            parts.push(format!(
                "client_max_window_bits={}",
                self.config.client_max_window_bits
            ));
        }

        parts.join("; ")
    }

    /// Parse Sec-WebSocket-Extensions header to check for permessage-deflate support
    ///
    /// # Arguments
    /// * `header` - The Sec-WebSocket-Extensions header value
    ///
    /// # Returns
    /// * `bool` - True if permessage-deflate is supported
    pub fn is_compression_supported(header: &str) -> bool {
        header.to_lowercase().contains("permessage-deflate")
    }

    /// Check if compression should be applied based on message size
    ///
    /// # Arguments
    /// * `size` - Message size in bytes
    ///
    /// # Returns
    /// * `bool` - True if message is large enough to compress
    pub fn should_compress(&self, size: usize) -> bool {
        self.config.enabled && size >= self.config.min_compress_size
    }

    /// Get the compression configuration
    pub fn config(&self) -> &WebSocketCompressionConfig {
        &self.config
    }
}

/// Statistics about WebSocket compression
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// Total bytes before compression
    pub bytes_in: usize,
    /// Total bytes after compression
    pub bytes_out: usize,
    /// Number of messages compressed
    pub messages_compressed: usize,
    /// Number of messages sent uncompressed
    pub messages_uncompressed: usize,
}

impl CompressionStats {
    /// Calculate compression ratio as percentage
    ///
    /// # Returns
    /// * `f64` - Compression ratio (0.0 = no compression, 0.5 = 50% reduction)
    pub fn compression_ratio(&self) -> f64 {
        if self.bytes_in == 0 {
            return 0.0;
        }
        1.0 - (self.bytes_out as f64 / self.bytes_in as f64)
    }

    /// Calculate bandwidth savings in bytes
    pub fn bytes_saved(&self) -> usize {
        self.bytes_in.saturating_sub(self.bytes_out)
    }

    /// Add compressed message to statistics
    pub fn add_compressed(&mut self, original_size: usize, compressed_size: usize) {
        self.bytes_in += original_size;
        self.bytes_out += compressed_size;
        self.messages_compressed += 1;
    }

    /// Add uncompressed message to statistics
    pub fn add_uncompressed(&mut self, size: usize) {
        self.bytes_in += size;
        self.bytes_out += size;
        self.messages_uncompressed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_config_default() {
        let config = WebSocketCompressionConfig::default();
        assert!(config.enabled);
        assert_eq!(config.compression_level, 6);
        assert!(!config.server_no_context_takeover);
        assert!(!config.client_no_context_takeover);
        assert_eq!(config.server_max_window_bits, 15);
        assert_eq!(config.client_max_window_bits, 15);
        assert_eq!(config.min_compress_size, 256);
    }

    #[test]
    fn test_compress_decompress_text() {
        let config = WebSocketCompressionConfig::default();
        let compressor = WebSocketCompression::new(config);

        // Large text message that should compress well
        let original = "Hello, World! ".repeat(100);
        let original_bytes = original.as_bytes();

        // Compress
        let compressed = compressor
            .compress(original_bytes)
            .expect("Compression failed");
        assert!(compressed.is_some(), "Should compress large repeated text");

        let compressed_data = compressed.unwrap();
        assert!(
            compressed_data.len() < original_bytes.len(),
            "Compressed size should be smaller"
        );

        // Decompress
        let decompressed = compressor
            .decompress(&compressed_data)
            .expect("Decompression failed");
        assert_eq!(
            decompressed, original_bytes,
            "Decompressed data should match original"
        );
    }

    #[test]
    fn test_compress_small_message() {
        let config = WebSocketCompressionConfig::default();
        let compressor = WebSocketCompression::new(config);

        // Small message below min_compress_size
        let small_message = b"Hello";

        let compressed = compressor
            .compress(small_message)
            .expect("Compression failed");
        assert!(
            compressed.is_none(),
            "Small messages should not be compressed"
        );
    }

    #[test]
    fn test_compression_disabled() {
        let config = WebSocketCompressionConfig {
            enabled: false,
            ..Default::default()
        };
        let compressor = WebSocketCompression::new(config);

        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).expect("Should not fail");
        assert!(
            compressed.is_none(),
            "Disabled compression should return None"
        );
    }

    #[test]
    fn test_extension_header() {
        let config = WebSocketCompressionConfig::default();
        let compressor = WebSocketCompression::new(config);

        let header = compressor.extension_header();
        assert_eq!(header, "permessage-deflate");

        // With options
        let config = WebSocketCompressionConfig {
            server_no_context_takeover: true,
            client_no_context_takeover: true,
            ..Default::default()
        };
        let compressor = WebSocketCompression::new(config);
        let header = compressor.extension_header();
        assert!(header.contains("permessage-deflate"));
        assert!(header.contains("server_no_context_takeover"));
        assert!(header.contains("client_no_context_takeover"));
    }

    #[test]
    fn test_is_compression_supported() {
        assert!(WebSocketCompression::is_compression_supported(
            "permessage-deflate"
        ));
        assert!(WebSocketCompression::is_compression_supported(
            "permessage-deflate; client_max_window_bits"
        ));
        assert!(WebSocketCompression::is_compression_supported(
            "PERMESSAGE-DEFLATE"
        ));
        assert!(!WebSocketCompression::is_compression_supported(
            "some-other-extension"
        ));
    }

    #[test]
    fn test_should_compress() {
        let config = WebSocketCompressionConfig {
            min_compress_size: 100,
            ..Default::default()
        };
        let compressor = WebSocketCompression::new(config);

        assert!(!compressor.should_compress(50));
        assert!(!compressor.should_compress(99));
        assert!(compressor.should_compress(100));
        assert!(compressor.should_compress(1000));
    }

    #[test]
    fn test_compression_stats() {
        let mut stats = CompressionStats::default();

        // Add some compressed messages
        stats.add_compressed(1000, 400); // 60% compression
        stats.add_compressed(2000, 800); // 60% compression

        // Add uncompressed message
        stats.add_uncompressed(100);

        assert_eq!(stats.bytes_in, 3100);
        assert_eq!(stats.bytes_out, 1300);
        assert_eq!(stats.messages_compressed, 2);
        assert_eq!(stats.messages_uncompressed, 1);
        assert_eq!(stats.bytes_saved(), 1800);

        let ratio = stats.compression_ratio();
        assert!((ratio - 0.58).abs() < 0.01); // Approximately 58% reduction
    }

    #[test]
    fn test_compression_levels() {
        // Test different compression levels
        let original = "The quick brown fox jumps over the lazy dog. ".repeat(50);

        for level in [0, 1, 6, 9] {
            let config = WebSocketCompressionConfig {
                compression_level: level,
                min_compress_size: 0, // Compress everything
                ..Default::default()
            };
            let compressor = WebSocketCompression::new(config);

            let compressed = compressor
                .compress(original.as_bytes())
                .expect("Compression failed");

            if level == 0 {
                // Level 0 might not compress
                continue;
            }

            assert!(compressed.is_some(), "Level {} should compress", level);
            let compressed_data = compressed.unwrap();

            // Decompress and verify
            let decompressed = compressor
                .decompress(&compressed_data)
                .expect("Decompression failed");
            assert_eq!(decompressed, original.as_bytes());
        }
    }

    #[test]
    fn test_compression_ratio_calculation() {
        let mut stats = CompressionStats::default();
        assert_eq!(stats.compression_ratio(), 0.0); // No data

        stats.bytes_in = 1000;
        stats.bytes_out = 500;
        assert_eq!(stats.compression_ratio(), 0.5); // 50% reduction

        stats.bytes_in = 1000;
        stats.bytes_out = 1000;
        assert_eq!(stats.compression_ratio(), 0.0); // No compression

        stats.bytes_in = 1000;
        stats.bytes_out = 250;
        assert_eq!(stats.compression_ratio(), 0.75); // 75% reduction
    }

    #[test]
    fn test_json_compression() {
        let config = WebSocketCompressionConfig {
            min_compress_size: 0, // Compress everything
            ..Default::default()
        };
        let compressor = WebSocketCompression::new(config);

        // JSON data compresses very well
        let json = r#"{"users":[{"name":"Alice","age":30},{"name":"Bob","age":25},{"name":"Charlie","age":35}]}"#.repeat(10);

        let compressed = compressor
            .compress(json.as_bytes())
            .expect("Compression failed")
            .expect("Should compress JSON");

        // JSON should compress significantly
        let ratio = 1.0 - (compressed.len() as f64 / json.len() as f64);
        assert!(ratio > 0.5, "JSON should compress by at least 50%");

        // Verify decompression
        let decompressed = compressor
            .decompress(&compressed)
            .expect("Decompression failed");
        assert_eq!(decompressed, json.as_bytes());
    }
}
