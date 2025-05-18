# SNIProxy-rs

A high-performance SNI proxy implementation in Rust focusing on efficiency and reliability. Routes traffic based on SNI (HTTPS) and Host headers (HTTP).

## Core Features

- Fast, asynchronous connection handling
- SNI-based routing for HTTPS
- Host header-based routing for HTTP
- Prometheus metrics integration
- JSON-structured logging
- Configurable connection timeouts
- Multi-port listening capability
- Domain allowlist support
- Minimal memory footprint
- Zero-copy data transfer where possible

## Architecture

Built using modern Rust async/await with:
- Tokio for async runtime
- Hyper for HTTP handling
- Custom TLS parsing for minimal overhead
- Prometheus for metrics
- Configuration via YAML
- Structured logging with tracing

## Building

Requires Rust 1.70 or newer.

```bash
# Clone the repository
git clone https://github.com/yourusername/sniproxy-rs.git
cd sniproxy-rs

# Build
cargo build --release
```

## Configuration

Create `config.yaml`:

```yaml
timeouts:
  connect: 10
  client_hello: 10
  idle: 300

listen_addrs:
  - "0.0.0.0:80"
  - "0.0.0.0:443"

metrics:
  enabled: true
  address: "127.0.0.1:9000"

allowlist:
  - "example.com"
  - "*.example.org"
```

## Running

```bash
# Development
cargo run -- -c config.yaml

# Production
./target/release/sniproxy-server -c config.yaml
```

## Performance Tuning

For optimal performance:

```bash
# /etc/sysctl.conf
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
```

## Metrics

Available at `http://<metrics_address>/metrics`:
- Connection counters
- Bytes transferred
- Connection duration histograms

## Contributing

Contributions welcome! Please read CONTRIBUTING.md for guidelines.

## License

This project is licensed under the MIT License - see LICENSE for details.
