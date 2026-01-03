# SNIProxy-rs Configuration Analysis
**Analysis Date**: 2026-01-03
**Status**: ✅ Config is functional but missing optional parameters

---

## Executive Summary

The configuration system is working correctly with all required parameters present. However, several optional configuration sections are missing from `config.yaml` that could provide additional control over advanced features.

**Findings**:
- ✅ All required config parameters present
- ⚠️ 4 optional config sections missing (but have reasonable defaults)
- ⚠️ Several internal configs not exposed (may need exposure for tuning)
- ⚠️ Some hardcoded constants that could be configurable

---

## Current Config Status

### ✅ Present in config.yaml
1. **listen_addrs** - TCP listen addresses (required)
2. **timeouts** - Connection timeouts (required)
3. **metrics** - Prometheus metrics config (required)
4. **max_connections** - Connection limit (optional, defaults to 10000)
5. **shutdown_timeout** - Graceful shutdown timeout (optional, defaults to 30s)
6. **connection_pool** - Connection pooling config (optional)
7. **allowlist** - Domain allowlist (optional, currently commented out)

### ⚠️ Missing Optional Sections

#### 1. Protocol Routing Configuration
**Section**: `protocol_routing`
**Purpose**: Configure web protocol detection (Socket.IO, JSON-RPC, XML-RPC, SOAP, RPC)

**Available options**:
```yaml
protocol_routing:
  socketio:
    enabled: true
    extract_from_path: true
    polling_timeout: 30

  jsonrpc:
    enabled: true
    validate_batch: true
    max_batch_size: 100

  xmlrpc:
    enabled: true
    validate_xml: true

  soap:
    enabled: true
    extract_from_action: true
    validate_wsdl: false

  rpc:
    enabled: true
    detect_from_path: true
```

**Impact**: Uses defaults (all enabled). No user control over protocol detection.

---

#### 2. UDP/QUIC Listener Addresses
**Section**: `udp_listen_addrs`
**Purpose**: Enable HTTP/3 and QUIC protocol support

**Available options**:
```yaml
udp_listen_addrs:
  - "0.0.0.0:443"      # QUIC/HTTP3 traffic
  - "[::]:443"         # IPv6 QUIC/HTTP3
```

**Impact**: UDP/QUIC not currently enabled. No HTTP/3 support without this.

---

#### 3. QUIC Protocol Configuration
**Section**: `quic_config`
**Purpose**: Configure QUIC transport settings

**Available options**:
```yaml
quic_config:
  enabled: true
  max_concurrent_streams: 100      # Per-connection stream limit
  max_idle_timeout: 60             # Seconds
  keep_alive_interval: 15          # Seconds
  max_datagram_size: 1350          # Bytes (MTU safe)
  enable_0rtt: true                # 0-RTT resumption
```

**Impact**: Uses defaults. No fine-tuning of QUIC behavior.

---

#### 4. HTTP/3 Configuration
**Section**: `http3_config`
**Purpose**: Configure HTTP/3 protocol behavior

**Available options**:
```yaml
http3_config:
  enabled: true
  max_field_section_size: 8192           # Header size limit
  qpack_max_table_capacity: 4096         # QPACK compression table
  qpack_blocked_streams: 16              # QPACK decoder limit
```

**Impact**: Uses defaults. No control over HTTP/3 header compression.

---

## Internal Configurations (Not Exposed)

These configurations exist in code but are NOT exposed in the main Config struct. They use hardcoded defaults.

### 1. gRPC Connection Pool
**Location**: `sniproxy-core/src/grpc_pool.rs`

**Defaults**:
```rust
max_channels_per_host: 10
channel_ttl: 300           // 5 minutes
idle_timeout: 120          // 2 minutes
enabled: true
max_concurrent_streams: 100
health_check_interval: 30  // seconds
```

**Recommendation**: Consider exposing via config for performance tuning.

---

### 2. HTTP/2 Push Cache
**Location**: `sniproxy-core/src/http2_cache.rs`

**Defaults**:
```rust
enabled: true
max_entries: 1000
ttl: 300                   // 5 minutes
auto_cleanup: true
```

**Recommendation**: Could expose for memory/performance tuning.

---

### 3. WebSocket Compression
**Location**: `sniproxy-core/src/websocket_compression.rs`

**Defaults**:
```rust
enabled: true
compression_level: 6       // 0-9 scale
server_no_context_takeover: false
client_no_context_takeover: false
server_max_window_bits: 15
client_max_window_bits: 15
min_compress_size: 256     // bytes
```

**Recommendation**: Consider exposing compression_level and min_compress_size.

---

### 4. QPACK (HTTP/3 Header Compression)
**Location**: `sniproxy-core/src/qpack.rs`

**Defaults**:
```rust
max_table_capacity: 4096
max_blocked_streams: 16
enabled: true
```

**Note**: Already exposed via http3_config section.

---

## Hardcoded Constants

These are performance-critical constants that are currently hardcoded.

### Buffer Sizes

#### In connection.rs (lines 17-20)
```rust
const MAX_TLS_HEADER_SIZE: usize = 16384;  // 16KB
const MIN_TLS_HEADER_SIZE: usize = 5;
const PEEK_SIZE: usize = 24;
const COPY_BUFFER_SIZE: usize = 32768;     // 32KB
```

**Rationale**: Optimized for performance. Rarely needs changing.
**Recommendation**: ✅ Keep hardcoded unless users report issues.

---

#### In http.rs (lines 11-12)
```rust
const READ_BUFFER_SIZE: usize = 16384;     // 16KB
const COPY_BUFFER_SIZE: usize = 32768;     // 32KB
```

**Rationale**: Tuned for HTTP performance.
**Recommendation**: ✅ Keep hardcoded.

---

### Timeouts

#### In http.rs
```rust
let response_timeout = Duration::from_secs(10);    // Line 151
let detection_timeout = Duration::from_secs(5);    // Lines 309, 363
```

**Issue**: ⚠️ These timeouts are hardcoded but could be useful in config.
**Recommendation**: Consider adding to `timeouts` section:
```yaml
timeouts:
  connect: 10
  client_hello: 5
  idle: 300
  http_response: 10      # NEW: HTTP response timeout
  protocol_detection: 5   # NEW: Protocol detection timeout
```

---

#### In udp_connection.rs (lines 52-58)
```rust
const MAX_DATAGRAM_SIZE: usize = 1350;
const SESSION_TIMEOUT_SECS: u64 = 30;
const MAX_SESSIONS: usize = 10_000;
```

**Issue**: ⚠️ These UDP/QUIC settings are hardcoded.
**Recommendation**: Should be in `quic_config`:
```yaml
quic_config:
  max_datagram_size: 1350      # Already available
  session_timeout: 30           # NEW: Should add to QuicConfig
  max_sessions: 10000           # NEW: Should add to QuicConfig
```

---

### Default Ports

#### In connection.rs (lines 93-95)
```rust
Protocol::Http10 | Protocol::Http11 | Protocol::WebSocket => 80,
Protocol::Http2 | Protocol::Grpc | Protocol::Tls => 443,
Protocol::Http3 => 443,
```

**Rationale**: ✅ Standard protocol defaults. Should remain hardcoded.
**Recommendation**: Keep as-is. These are well-known port conventions.

---

## Recommendations

### Priority: High

1. **Add Optional Sections to config.yaml**
   - Add commented examples of `protocol_routing`, `udp_listen_addrs`, `quic_config`, `http3_config`
   - Provides documentation even if not actively used
   - Users can uncomment to enable HTTP/3 or tune protocol detection

2. **Add Missing Timeout Configs**
   - `http_response` timeout (currently hardcoded at 10s)
   - `protocol_detection` timeout (currently hardcoded at 5s)
   - Add to `timeouts` section in config struct

### Priority: Medium

3. **Expose Internal Configs** (if needed for tuning)
   - Add `grpc_pool` section to main Config
   - Add `http2_cache` section to main Config
   - Add `websocket_compression` section to main Config

4. **Add UDP/QUIC Session Limits**
   - Add `session_timeout` to QuicConfig
   - Add `max_sessions` to QuicConfig

### Priority: Low

5. **Buffer Size Configuration** (optional, for advanced users)
   - Most users won't need to change these
   - Could add `performance_tuning` section if needed

---

## Updated Config Example

See `config.yaml.example` for a complete configuration file with all available options documented.

---

## Verification Checklist

- ✅ All required config parameters present in config.yaml
- ✅ Connection pooling configured and working
- ✅ Metrics configured correctly (port 9090)
- ✅ Timeouts configured appropriately
- ⚠️ HTTP/3 not enabled (needs udp_listen_addrs)
- ⚠️ Protocol routing using defaults (not configurable via file)
- ⚠️ Some internal timeouts hardcoded (http_response, protocol_detection)
- ⚠️ UDP/QUIC session limits hardcoded

---

## Conclusion

**Current Status**: ✅ Config is functional and production-ready for TCP/HTTP/HTTPS traffic.

**For HTTP/3 Support**: Add `udp_listen_addrs`, `quic_config`, and `http3_config` sections.

**For Advanced Tuning**: Consider exposing internal configs (gRPC pool, HTTP/2 cache, WebSocket compression).

**No Critical Issues**: All hardcoded values have reasonable defaults. The service works correctly without additional configuration.

---

**Analysis Performed**: 2026-01-03
**Analyzed Files**:
- `config.yaml`
- `sniproxy-config/src/lib.rs`
- All `*_config` structs in sniproxy-core
- Connection handling code for hardcoded constants
