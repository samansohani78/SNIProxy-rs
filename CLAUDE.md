# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SNIProxy-rs is a high-performance SNI (Server Name Indication) proxy written in Rust. It routes traffic based on SNI for HTTPS connections and Host headers for HTTP connections. The proxy supports multiple protocols including HTTP/1.0, HTTP/1.1, HTTP/2, HTTP/3, WebSocket, and gRPC.

## Workspace Structure

This is a Cargo workspace with four crates:

- **sniproxy-config**: Configuration parsing and types (YAML-based config)
- **sniproxy-core**: Core proxy logic including TLS parsing, SNI extraction, connection handling, and protocol detection
- **sniproxy-bin**: Binary entry point and metrics HTTP server
- **sniproxy**: Top-level convenience crate that re-exports the above

The binary is named `sniproxy-server` (defined in sniproxy-bin/Cargo.toml).

## Common Commands

### Building
```bash
# Development build
cargo build

# Release build (recommended for performance testing)
cargo build --release

# Build specific crate
cargo build -p sniproxy-core
```

### Testing
```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p sniproxy-core

# Run specific test
cargo test test_extract_sni_simple

# Run tests with logging output
RUST_LOG=debug cargo test -- --nocapture
```

### Code Quality
```bash
# Check for errors without building
cargo check

# Run clippy linter
cargo clippy -- -D warnings

# Check formatting
cargo fmt -- --check

# Auto-format code
cargo fmt
```

### Benchmarking
```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench sni_extraction

# Generate HTML benchmark report
cargo bench -- --save-baseline main
```

### Documentation
```bash
# Generate and open documentation
cargo doc --open

# Generate documentation for all workspace members
cargo doc --workspace --no-deps

# Test documentation examples
cargo test --doc
```

### Examples
```bash
# List all examples
cargo run --example

# Run basic proxy example
cargo run --example basic_proxy

# Run proxy with metrics
cargo run --example proxy_with_metrics

# Run SNI extraction demo
cargo run --example sni_extraction

# Run config loading example
cargo run --example config_loading
```

### Running the Proxy
```bash
# Development run with custom config
cargo run -- -c config.yaml

# Production run
./target/release/sniproxy-server -c config.yaml

# With debug logging
RUST_LOG=sniproxy=debug cargo run -- -c config.yaml
```

## Architecture

### Connection Flow

1. **Listener** (sniproxy-core/src/lib.rs:run_proxy): Accepts TCP connections on configured listen addresses
2. **Protocol Detection** (sniproxy-core/src/connection.rs:ConnectionHandler): Peeks at initial bytes to determine protocol type
3. **Routing Decision**:
   - For TLS traffic: Extracts SNI from ClientHello handshake
   - For HTTP traffic: Reads Host header from HTTP request
   - For HTTP/2: Detects via ALPN extension or HTTP/2 preface
4. **Tunneling**: Establishes connection to target host and proxies bidirectional traffic with optional metrics tracking

### Protocol Detection Strategy

The proxy uses a sophisticated protocol detection mechanism in `ConnectionHandler`:

1. **Peek Phase**: Read first PEEK_SIZE (24) bytes without consuming from stream
2. **TLS Detection**: Check for TLS handshake byte (0x16) and version
3. **HTTP Detection**: Check for HTTP method prefixes (GET, POST, etc.)
4. **HTTP/2 Detection**: Look for HTTP/2 preface or h2/h3 ALPN protocols
5. **Fallback**: Treat unknown as generic TLS traffic

### Key Components

- **TLS Parsing** (sniproxy-core/src/lib.rs:extract_sni): Custom zero-copy TLS ClientHello parser that extracts SNI without using heavyweight TLS libraries
- **ALPN Extraction** (sniproxy-core/src/lib.rs:extract_alpn): Detects application protocol (h2, h3) from TLS extension
- **HTTP Handling** (sniproxy-core/src/http.rs): Extracts Host header, handles WebSocket upgrades, gRPC detection
- **Metrics** (sniproxy-core/src/connection.rs:ConnectionMetrics): Comprehensive Prometheus metrics including:
  - `sniproxy_connections_total` - Total connections by protocol and status
  - `sniproxy_connections_active` - Currently active connections (gauge)
  - `sniproxy_connection_duration_seconds` - Connection duration histogram
  - `sniproxy_errors_total` - Errors by type and protocol
  - `sniproxy_protocol_distribution_total` - Protocol usage distribution
  - `sniproxy_bytes_transferred_total` - Bytes transferred per host/direction
- **Health Check** (sniproxy-bin/src/lib.rs): Kubernetes-ready endpoints at `/health`, `/metrics`, and `/`
- **Allowlist** (sniproxy-config): Domain pattern matching with wildcard support

### Timeouts

Three configurable timeouts in config.yaml:
- `client_hello`: Time allowed to receive initial TLS ClientHello or HTTP headers
- `connect`: Time allowed to establish backend connection
- `idle`: Maximum connection idle time before closing

## Configuration

The config file (config.yaml) structure is defined in sniproxy-config/src/lib.rs. Required fields:
- `listen_addrs`: Array of "ip:port" strings
- `timeouts`: connect, client_hello, idle (all in seconds)
- `metrics`: enabled (bool), address (string)
- `allowlist`: Optional array of domain patterns (supports wildcards like "*.example.com")

## Testing Infrastructure

### Unit Tests (39 tests total)
- **sniproxy-config** (9 tests): Config parsing, validation, allowlist pattern matching
- **sniproxy-core/lib.rs** (12 tests): SNI extraction, ALPN parsing, error handling
- **sniproxy-core/http.rs** (13 tests): HTTP header parsing, Host extraction
- **Integration tests** (5 tests): End-to-end functionality tests

### Benchmarks
Performance benchmarks using Criterion are located in `sniproxy-core/benches/`:
- `sni_parsing.rs`: Benchmarks for SNI and ALPN extraction with various input sizes

### Manual Testing
The test/ directory contains nginx-based integration tests:
- `test/http1/`: HTTP/1.x test setup with nginx config and HTML
- `test/http2/`: HTTP/2 test setup with TLS certificates and nginx config

### Examples
The `examples/` directory contains practical usage examples:
- `basic_proxy.rs`: Minimal proxy setup
- `proxy_with_metrics.rs`: Proxy with Prometheus metrics and allowlist
- `sni_extraction.rs`: Demonstrates SNI/ALPN extraction functions
- `config_loading.rs`: Configuration loading and validation examples

## Commit Message Format

From CONTRIBUTING.md, use this format:
```
category: short description

Detailed description of changes and reasoning.
```

Categories: feat, fix, perf, docs, test, refactor, chore

## Code Style Notes

- Follow standard Rust conventions (rustfmt and clippy)
- Use structured logging with tracing macros (info!, debug!, error!, warn!)
- Keep zero-copy optimizations where possible (e.g., TLS parsing uses byte slices)
- Async/await with Tokio runtime throughout
- Metrics are optional - code should work with metrics disabled
- **All public APIs must have rustdoc comments** with examples
- Function documentation should include: description, arguments, returns, errors (if applicable), and usage examples

## Documentation

All public APIs are documented with rustdoc comments including:
- Function descriptions and purpose
- Parameter documentation
- Return value documentation
- Error conditions (where applicable)
- Usage examples

View documentation with: `cargo doc --open`

## Production Monitoring

### Prometheus Metrics

The proxy exposes comprehensive Prometheus metrics on the configured metrics address (default: `0.0.0.0:9000`):

**Connection Metrics:**
```promql
# Active connections
sniproxy_connections_active

# Total connections by protocol and status
sniproxy_connections_total{protocol="http1.1",status="success"}

# Connection duration percentiles
histogram_quantile(0.95, rate(sniproxy_connection_duration_seconds_bucket[5m]))
```

**Error Tracking:**
```promql
# Error rate by type
rate(sniproxy_errors_total{error_type="connection"}[5m])
```

**Protocol Analytics:**
```promql
# Protocol distribution
sum by (protocol) (rate(sniproxy_protocol_distribution_total[5m]))
```

**Data Transfer:**
```promql
# Throughput by host
sum by (host) (rate(sniproxy_bytes_transferred_total[5m]))
```

### Health Check Endpoints

- `/health` - Returns `{"status":"healthy","service":"sniproxy"}` for K8s liveness/readiness probes
- `/metrics` - Prometheus metrics in text exposition format
- `/` - Returns list of available endpoints

**Kubernetes Integration:**
```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 9000
  initialDelaySeconds: 5
  periodSeconds: 10
```

See `PHASE3_SUMMARY.md` for complete monitoring setup including Grafana dashboards and alerting rules.

## Performance Optimizations

### Hot Path Optimizations (Phase 4)

The codebase includes several performance optimizations:

**Buffer Sizing:**
- HTTP read buffer: 16KB (optimal for typical headers)
- HTTP copy buffer: 32KB (high throughput)
- TLS record buffer: 16KB (RFC maximum)
- All buffers pre-allocated to avoid reallocations

**Inline Hints:**
- 12 hot functions marked with `#[inline]` for zero-cost abstractions
- Protocol detection, header parsing, and metrics copy all inlined

**String Allocation Reduction:**
- Static string references for metric labels (70% allocation reduction)
- Case-insensitive header parsing without allocating lowercase copies
- Windows iterator for pattern matching (SIMD-optimizable)

**Performance Characteristics:**
- SNI extraction: 500ns - 2μs depending on domain length
- Connection setup: <1ms typical (including DNS)
- Throughput: Network-bound, not CPU-bound
- Memory: ~50KB per active connection
- Concurrency: 10,000+ connections tested

See `PERFORMANCE_OPTIMIZATIONS.md` for detailed optimization techniques and benchmarking methodology.

## Documentation Files

- **CLAUDE.md** - This file, development guide
- **TEST_SUMMARY.md** - Phase 1 testing infrastructure summary
- **IMPROVEMENTS_SUMMARY.md** - Phase 2 enhancements (benchmarks, docs, examples)
- **PHASE3_SUMMARY.md** - Production monitoring and observability guide
- **PERFORMANCE_OPTIMIZATIONS.md** - Phase 4 performance tuning details

## CI/CD

GitHub Actions workflow (`.github/workflows/ci.yml`) runs on push/PR:
- Multi-platform testing (Ubuntu, macOS, Windows)
- Multi-version Rust testing (stable, beta)
- Clippy linting
- Format checking
- Security audits
- Code coverage reporting
