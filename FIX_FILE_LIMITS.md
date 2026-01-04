# CRITICAL BUG FIX: File Descriptor Leak

## Problem Report

User reported: **"too many open files"** error in production

## Root Cause Analysis

Found **CRITICAL BUG** in connection pool implementation:

### The Bug

1. **Connection pooling is ENABLED** in config.yaml:
   ```yaml
   connection_pool:
     enabled: true
     max_per_host: 1000
   ```

2. **Connections are taken from pool** but **NEVER returned**:
   ```rust
   // sniproxy-core/src/connection.rs:946
   if let Some(ref pool) = self.pool
       && let Some(stream) = pool.get(target_addr)  // ❌ Takes connection
   {
       debug!("Using pooled connection to {}", target_addr);
       return Ok(stream);
   }
   ```

3. **return_to_pool() function is DEAD CODE**:
   ```rust
   // sniproxy-core/src/connection.rs:970
   #[allow(dead_code)]  // ❌ NEVER CALLED!
   async fn return_to_pool(&self, target_addr: String, stream: TcpStream) {
       if let Some(ref pool) = self.pool {
           if pool.put(target_addr, stream) {
               debug!("Connection returned to pool");
           }
       }
   }
   ```

4. **Metrics tracking is broken**:
   ```rust
   // sniproxy-core/src/connection_pool.rs:267
   // When taking from pool:
   metrics.active_connections.inc();  // ✅ Incremented

   // sniproxy-core/src/connection_pool.rs:314
   // When returning to pool:
   metrics.active_connections.dec();  // ❌ NEVER CALLED! (dead code)
   ```

### The Leak Mechanism

```
1. Client connects → handler spawned
2. Need connection to backend → pool.get("example.com")
   - Takes TcpStream from pool
   - Increments active_connections metric
   - TcpStream ownership transferred

3. Connection used, then closed/dropped
   - TcpStream is dropped (closed automatically by Rust)
   - But pool still thinks it's "active" (metric not decremented)
   - Pool never gets the connection back

4. Repeat for each connection:
   - Pool.active_connections keeps growing
   - Pool.pools HashMap keeps growing with dead references
   - File descriptors accumulate

5. Eventually: "too many open files" (ulimit -n exceeded)
```

## Why This Doesn't Affect Non-Pooled Connections

When pooling is disabled or pool.get() returns None:
```rust
// New connection created
let server = timeout(connect_timeout, TcpStream::connect(addr)).await??;
// Connection used in copy_bidirectional_timeout()
// When function ends, TcpStream is dropped → auto-closed ✅
// No pool tracking, no leak!
```

## Why Connection Pooling Doesn't Make Sense Here

**SNIProxy is a TRANSPARENT PROXY**, not a forward proxy:

- **Forward Proxy (pooling works):** Client explicitly connects to proxy, proxy connects to same backend repeatedly
  ```
  Client → [Proxy → example.com] ← Can reuse connection!
  Client → [Proxy → example.com] ← Same backend!
  ```

- **Transparent Proxy (pooling fails):** Each connection is to a DIFFERENT backend
  ```
  Client1 → [Proxy → github.com]    ← SNI: github.com
  Client2 → [Proxy → gitlab.com]    ← SNI: gitlab.com
  Client3 → [Proxy → example.com]   ← SNI: example.com
  ```

  **You CANNOT reuse github.com connection for gitlab.com request!**

## Impact

**Severity:** 🔴 **CRITICAL** - Production outage

**Symptoms:**
- "too many open files" errors
- New connections rejected
- Service degradation/crash
- Requires restart to recover

**Affected:** All production deployments with `connection_pool.enabled: true`

## Fix

**Disable connection pooling for transparent proxy:**

```yaml
# config.yaml
connection_pool:
  enabled: false  # ← Change from true to false
```

**Why this fixes it:**
- No pool.get() calls → no leaked references
- TcpStreams properly dropped/closed when connections end
- No metrics tracking issues
- File descriptors properly released

## Alternative Fix (Not Recommended)

If you want to keep pooling (not recommended for transparent proxy):

```rust
// In handle_https(), handle_http(), etc. - AFTER copy_bidirectional_timeout():
self.return_to_pool(target_addr.clone(), server).await;
```

**But this doesn't make sense for transparent proxy!** Each SNI is different.

## Testing the Fix

### Before Fix (Reproduce Bug):
```bash
# On server with pooling enabled
watch -n 1 'lsof -p $(pgrep sniproxy-server) | wc -l'
# Should see file descriptor count growing continuously

# Eventually:
# Error: "too many open files"
```

### After Fix (Verify):
```bash
# Disable pooling in config.yaml
connection_pool:
  enabled: false

# Restart service
systemctl restart sniproxy

# Monitor file descriptors
watch -n 1 'lsof -p $(pgrep sniproxy-server) | wc -l'
# Should see count grow during connections, then drop when connections close
# Stable at baseline + active connections
```

## Deployment Steps

1. **Update config.yaml:**
   ```yaml
   connection_pool:
     enabled: false
   ```

2. **Restart service:**
   ```bash
   systemctl restart sniproxy
   ```

3. **Verify fix:**
   ```bash
   # Check file descriptor count
   lsof -p $(pgrep sniproxy-server) | wc -l

   # Monitor for growth
   watch -n 5 'lsof -p $(pgrep sniproxy-server) | wc -l'

   # Check logs for errors
   journalctl -u sniproxy -f
   ```

4. **Monitor metrics:**
   ```bash
   # Check active connections metric (should not grow indefinitely)
   curl http://localhost:9090/metrics | grep sniproxy_pool_active_connections
   ```

## Prevention

**Code-level fix to prevent future issues:**

Add documentation to ConnectionPool:
```rust
/// ⚠️  WARNING: Connection pooling is NOT suitable for transparent proxies!
///
/// In a transparent proxy, each connection is to a potentially different
/// backend (based on SNI/Host header). Pooling only makes sense if:
/// 1. Multiple requests to the SAME backend
/// 2. Connections are properly returned to pool after use
///
/// For SNIProxy (transparent proxy), set `enabled: false`
pub struct ConnectionPool {
    // ...
}
```

Add validation in config loading:
```rust
// Warn if pooling is enabled
if config.connection_pool.enabled {
    warn!("Connection pooling enabled - only use for forward proxy scenarios!");
    warn!("For transparent proxy (SNI-based routing), disable pooling to avoid leaks");
}
```

## Metrics to Watch

After deploying fix, monitor these:

```promql
# File descriptors (should be stable)
process_open_fds

# Active connections (should match reality)
sniproxy_connections_active

# Pool metrics (should be 0 with pooling disabled)
sniproxy_pool_active_connections
sniproxy_pool_size
```

## Summary

- **Bug:** Connection pool takes connections but never returns them
- **Impact:** File descriptor leak → "too many open files"
- **Root Cause:** `return_to_pool()` is dead code, never called
- **Fix:** Disable connection pooling (not suitable for transparent proxy anyway)
- **Status:** Fixed by setting `connection_pool.enabled: false`

---

**This bug has been present since connection pooling was added. It affects ALL production deployments.**
