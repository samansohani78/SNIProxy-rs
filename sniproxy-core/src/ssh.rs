//! SSH protocol detection and routing
//!
//! This module provides SSH-specific functionality including:
//! - SSH version string parsing
//! - Username-based routing extraction
//! - Automatic destination detection from SSH username

use std::io::Error as IoError;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

/// Extract destination host from SSH username field
///
/// SSH doesn't have SNI-like functionality, but we can extract the destination
/// from the username field using these formats:
///
/// Format 1: user@host (e.g., git@github.com) - standard SSH format
/// Format 2: host (e.g., github.com) - just hostname, uses default user
///
/// # Examples
///
/// ```
/// use sniproxy_core::ssh::extract_ssh_destination;
///
/// // Format: git@github.com (standard)
/// assert_eq!(
///     extract_ssh_destination("git@github.com"),
///     ("github.com", "git")
/// );
///
/// // Format: github.com (hostname only, default user)
/// assert_eq!(
///     extract_ssh_destination("github.com"),
///     ("github.com", "root")
/// );
/// ```
pub fn extract_ssh_destination(username: &str) -> (&str, &str) {
    // Format 1: user@host (e.g., git@github.com) - standard SSH format
    if let Some(at_pos) = username.find('@') {
        let user = &username[..at_pos];
        let host = &username[at_pos + 1..];
        if !host.is_empty() && !user.is_empty() {
            return (host, user);
        }
    }

    // Format 2: just hostname (use default user "root")
    // Validate hostname: must not be empty, no whitespace, no @ symbol
    if !username.is_empty()
        && !username.contains(|c: char| c.is_whitespace())
        && !username.contains('@')
    {
        return (username, "root");
    }

    // Fallback: invalid format
    ("", "")
}

/// Parse SSH identification string from client
///
/// SSH connections start with an identification string like:
/// "SSH-2.0-OpenSSH_8.2p1 Ubuntu-4ubuntu0.5"
///
/// This function reads the first line to identify the SSH version.
pub async fn read_ssh_ident(stream: &mut TcpStream) -> Result<String, IoError> {
    let mut reader = BufReader::new(stream);
    let mut ident = String::new();

    // SSH ident must be received within reasonable time
    // Read until \r\n or \n
    reader.read_line(&mut ident).await?;

    Ok(ident.trim().to_string())
}

/// Extract username from SSH authentication attempts
///
/// This is a simplified parser that looks for the username in SSH
/// authentication messages. For a full implementation, you'd parse
/// the SSH binary protocol completely.
///
/// Returns None if username cannot be extracted yet.
#[allow(dead_code)]
pub async fn extract_ssh_username(
    _stream: &mut TcpStream,
    _initial_data: &[u8],
) -> Result<Option<String>, IoError> {
    // SSH protocol is binary after the version exchange
    // For now, we'll use a heuristic approach

    // Look for common SSH username patterns in the data
    // This is a simplified approach - full SSH parsing would be complex

    // If we have the SSH version string, we can try to read more data
    // and look for the username in SSH_MSG_USERAUTH_REQUEST

    // For initial implementation, return None - we'll rely on other routing methods
    // A full implementation would parse SSH packets to extract the username

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_destination_at_format() {
        // Standard SSH format: user@host
        assert_eq!(
            extract_ssh_destination("git@github.com"),
            ("github.com", "git")
        );
        assert_eq!(
            extract_ssh_destination("admin@server.example.com"),
            ("server.example.com", "admin")
        );
        assert_eq!(
            extract_ssh_destination("user@gitlab.com"),
            ("gitlab.com", "user")
        );
    }

    #[test]
    fn test_extract_destination_hostname_only() {
        // Hostname only - uses default "root" user
        assert_eq!(
            extract_ssh_destination("github.com"),
            ("github.com", "root")
        );
        assert_eq!(
            extract_ssh_destination("example.com"),
            ("example.com", "root")
        );
        assert_eq!(
            extract_ssh_destination("server.example.com"),
            ("server.example.com", "root")
        );
    }

    #[test]
    fn test_extract_destination_edge_cases() {
        // Empty string
        assert_eq!(extract_ssh_destination(""), ("", ""));

        // Just @ (invalid)
        assert_eq!(extract_ssh_destination("@"), ("", ""));

        // @ at start (invalid)
        assert_eq!(extract_ssh_destination("@github.com"), ("", ""));

        // @ at end (invalid)
        assert_eq!(extract_ssh_destination("git@"), ("", ""));

        // Whitespace (invalid)
        assert_eq!(extract_ssh_destination("git hub"), ("", ""));
    }

    #[test]
    fn test_complex_hostnames() {
        // Multi-level subdomains
        assert_eq!(
            extract_ssh_destination("git@git.server.example.com"),
            ("git.server.example.com", "git")
        );

        // IPv4 addresses
        assert_eq!(
            extract_ssh_destination("admin@192.168.1.1"),
            ("192.168.1.1", "admin")
        );

        // IPv6 would need brackets in URLs, but SSH doesn't require them
        assert_eq!(extract_ssh_destination("root@fe80::1"), ("fe80::1", "root"));
    }
}
