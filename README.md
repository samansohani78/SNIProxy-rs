# SNIProxy-rs

A high-performance transparent proxy written in Rust that routes traffic based on SNI (Server Name Indication) for HTTPS and Host headers for HTTP.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

## Features

- 🚀 **High Performance** - Built with Tokio async runtime for concurrent connection handling
- 🔒 **TLS Passthrough** - Routes HTTPS traffic based on SNI without terminating TLS
- 🌐 **HTTP Support** - Routes HTTP/1.x and HTTP/2 based on Host headers
- 📊 **Prometheus Metrics** - Built-in metrics endpoint for monitoring
- 🎯 **Protocol Detection** - Automatically detects HTTP/1.x, HTTP/2, WebSocket, gRPC
- 🛡️ **Domain Allowlist** - Optional whitelist for allowed domains
- ⚡ **Zero-Copy** - Efficient data transfer with minimal overhead
- 📝 **Structured Logging** - JSON-formatted logs with tracing support

## Quick Start

### Prerequisites

- Rust 1.70 or newer
- Linux/macOS/Windows
- Build tools (gcc/clang for linking)

### Installation

```bash
# Clone the repository
git clone https://github.com/samansohani78/SNIProxy-rs.git
cd SNIProxy-rs

# Build release binary
cargo build --release

# Binary will be at: ./target/release/sniproxy-server
```

### Configuration

Create a `config.yaml` file:

```yaml
# Listening addresses
listen_addrs:
  - "0.0.0.0:80"
  - "0.0.0.0:443"

# Timeout settings (in seconds)
timeouts:
  connect: 10        # Backend connection timeout
  client_hello: 10   # Time to receive TLS ClientHello or HTTP headers
  idle: 300          # Connection idle timeout

# Prometheus metrics
metrics:
  enabled: true
  address: "0.0.0.0:9000"

# Optional: Domain allowlist (supports wildcards)
allowlist:
  - "example.com"
  - "*.example.com"
```

### Running

```bash
# Run the proxy
./target/release/sniproxy-server -c config.yaml

# Or with custom log level
RUST_LOG=info ./target/release/sniproxy-server -c config.yaml
```

## How It Works

SNIProxy-rs acts as a transparent Layer 4/7 proxy:

1. **For HTTPS (port 443)**:
   - Peeks at TLS ClientHello to extract SNI (Server Name Indication)
   - Routes connection to the backend based on the hostname in SNI
   - Forwards all traffic transparently (no TLS termination)

2. **For HTTP (port 80)**:
   - Reads the Host header from HTTP request
   - Routes connection to the backend based on hostname
   - Supports HTTP/1.0, HTTP/1.1, HTTP/2 cleartext (h2c), WebSocket

3. **Protocol Detection**:
   - Automatically detects protocol type from initial bytes
   - Handles TLS, HTTP/1.x, HTTP/2, WebSocket, gRPC

## Architecture

```
Client → SNIProxy → Backend Server
         ↓
    [Extract SNI/Host]
         ↓
    [Route Decision]
         ↓
   [Bidirectional Forward]
```

### Workspace Structure

- `sniproxy-config` - Configuration parsing and types
- `sniproxy-core` - Core proxy logic, protocol detection, routing
- `sniproxy-bin` - Binary entry point and metrics server
- `sniproxy` - Top-level convenience crate

## Monitoring

### Metrics Endpoint

Access Prometheus metrics at `http://localhost:9000/metrics`:

```promql
# Key metrics
sniproxy_connections_total          # Total connections by protocol and status
sniproxy_connections_active         # Currently active connections
sniproxy_connection_duration_seconds # Connection duration histogram
sniproxy_bytes_transferred_total    # Bytes transferred per host
sniproxy_errors_total               # Error count by type
```

### Health Check

```bash
curl http://localhost:9000/health
# Returns: {"status":"healthy","service":"sniproxy"}
```

## Performance

Optimized for high throughput:

- **Async I/O** - Non-blocking connection handling with Tokio
- **Zero-Copy** - Direct TCP forwarding where possible
- **Efficient Parsing** - Custom TLS/HTTP parsers optimized for speed
- **Buffering** - Optimized buffer sizes (16KB-32KB) for different protocols

Typical performance:
- **Throughput**: Network-bound (multi-Gbps capable)
- **Latency**: <1ms connection setup overhead
- **Memory**: ~50KB per active connection
- **Concurrency**: 10,000+ concurrent connections tested

## Development

### Building

```bash
# Development build
cargo build

# Release build with optimizations
cargo build --release

# Run tests
cargo test --all

# Run benchmarks
cargo bench

# Check code quality
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
```

### Testing

```bash
# Unit and integration tests
cargo test --all

# Test specific protocol
cargo test test_http2

# Run with logging
RUST_LOG=debug cargo test -- --nocapture
```

## Deployment

### SystemD Service

See `install.sh` for automated installation:

```bash
sudo ./install.sh
```

This creates a systemd service at `/etc/systemd/system/sniproxy.service`.

### Docker

```bash
docker build -t sniproxy-rs .
docker run -p 80:80 -p 443:443 -v /path/to/config.yaml:/etc/sniproxy/config.yaml sniproxy-rs
```

## Use Cases

- **Development Proxy** - Route multiple local services by hostname
- **Load Balancer Frontend** - SNI-based routing to backend clusters
- **Multi-Tenant Proxy** - Route traffic for multiple domains
- **Protocol Gateway** - Handle HTTP/1.x, HTTP/2, WebSocket, gRPC uniformly

## Limitations

- **HTTP/3 (QUIC)** - Detection only, no full proxying (UDP-based, would require architectural changes)
- **TLS Termination** - Does not decrypt TLS traffic (by design - it's a passthrough proxy)
- **Connection Pooling** - Limited effectiveness due to transparent forwarding

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Areas for Contribution

- Additional protocol support
- Performance optimizations
- Documentation improvements
- Bug fixes and testing
- Feature requests

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with:
- [Tokio](https://tokio.rs/) - Async runtime
- [Hyper](https://hyper.rs/) - HTTP library
- [Prometheus](https://github.com/tikv/rust-prometheus) - Metrics
- [Tracing](https://github.com/tokio-rs/tracing) - Structured logging

---

**Note**: This is a transparent proxy. It does not terminate TLS or modify traffic. For a reverse proxy with TLS termination, consider nginx, HAProxy, or Traefik.
