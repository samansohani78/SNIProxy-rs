# SSH Proxy vs HTTP/HTTPS Proxy - Understanding the Difference

## The Problem You Encountered

You have this in `/etc/hosts`:
```
23.88.88.104	github.com
```

**This works perfectly for HTTP/HTTPS but NOT for SSH!** Here's why:

---

## HTTP/HTTPS Proxying (✅ Works with hosts file)

```
┌─────────────────────────────────────────┐
│  Your Machine                           │
│                                         │
│  1. Browser wants: github.com          │
│  2. DNS: github.com → 23.88.88.104    │ (/etc/hosts)
│  3. Connects to: 23.88.88.104:443      │
│  4. Sends: Host: github.com  OR        │
│            SNI: github.com             │  ← PROXY READS THIS!
└────────────┬────────────────────────────┘
             │
             ▼
    ┌────────────────────┐
    │  Proxy reads SNI   │
    │  "github.com"      │
    │  Routes to real    │
    │  github.com        │
    └────────────────────┘
```

**✅ Works because:** HTTPS ClientHello contains SNI (Server Name Indication) telling the proxy the real destination!

---

## SSH Proxying (❌ Doesn't work with hosts file alone)

```
┌─────────────────────────────────────────┐
│  Your Machine                           │
│                                         │
│  1. SSH wants: github.com              │
│  2. DNS: github.com → 23.88.88.104    │ (/etc/hosts)
│  3. Connects to: 23.88.88.104:22       │
│  4. Sends: SSH-2.0-OpenSSH_8.2        │ ← NO DESTINATION INFO!
└────────────┬────────────────────────────┘
             │
             ▼
    ┌────────────────────────────────┐
    │  Proxy checks:                 │
    │  - No SNI (SSH doesn't have)   │
    │  - No Host header              │
    │  - SO_ORIGINAL_DST = Self!     │  ← LOOP DETECTED!
    │  ❌ Connection rejected        │
    └────────────────────────────────┘
```

**❌ Fails because:** SSH protocol has NO hostname/destination info in the protocol itself!

---

## Solutions for SSH Proxying

### Solution 1: iptables REDIRECT (Recommended)

**How it works:**
```
┌─────────────────────────────────────────────┐
│  Your Machine                               │
│                                             │
│  1. SSH wants: github.com (real IP)        │
│  2. Kernel routes to: real-github-ip:22    │
│  3. iptables intercepts & redirects to:    │
│     23.88.88.104:22                        │
│  4. iptables SAVES original dest in        │
│     SO_ORIGINAL_DST socket option          │ ← KEY POINT!
└──────────────┬──────────────────────────────┘
               │ Original dest: real-github-ip:22
               │ Actual connection: 23.88.88.104:22
               ▼
      ┌─────────────────────────┐
      │  Proxy reads:           │
      │  SO_ORIGINAL_DST =      │
      │  real-github-ip:22      │ ← PROXY KNOWS WHERE TO GO!
      │  ✅ Routes correctly    │
      └─────────────────────────┘
```

**Setup:**
```bash
# DON'T use /etc/hosts for github.com
# Remove: 23.88.88.104  github.com

# Instead, use iptables to redirect ALL SSH
sudo iptables -t nat -A OUTPUT -p tcp --dport 22 ! -d 23.88.88.104 -j DNAT --to-destination 23.88.88.104:22
```

**Advantages:**
- ✅ Works with ANY SSH destination (GitHub, GitLab, your servers, etc.)
- ✅ Fully automatic
- ✅ No per-host configuration

**Disadvantages:**
- Linux only
- Requires root to set up iptables

---

### Solution 2: SSH ProxyCommand (Universal)

**How it works:**
```
┌─────────────────────────────────────────┐
│  Your Machine                           │
│                                         │
│  ~/.ssh/config:                        │
│  Host github.com                       │
│    ProxyCommand nc 23.88.88.104 22    │ ← Explicit routing
│                                         │
│  1. SSH wants: github.com              │
│  2. SSH config says: use ProxyCommand  │
│  3. nc connects to proxy               │
│  4. SSH tells nc: connect to github.com│ ← DESTINATION PRESERVED!
└──────────────┬────────────────────────────┘
               │ Destination: github.com:22
               ▼
      ┌─────────────────────┐
      │  Proxy sees the     │
      │  destination from   │
      │  the netcat tunnel  │
      │  ✅ Routes correctly│
      └─────────────────────┘
```

**Setup:**
```bash
# DON'T use /etc/hosts for github.com

# Add to ~/.ssh/config:
Host github.com
    ProxyCommand nc -X connect -x 23.88.88.104:22 %h %p

Host gitlab.com
    ProxyCommand nc -X connect -x 23.88.88.104:22 %h %p
```

**Advantages:**
- ✅ Works on all operating systems
- ✅ No root required
- ✅ Fine-grained control per host

**Disadvantages:**
- Need to configure each destination
- Not automatic for all SSH

---

### Solution 3: Port-Based Manual Routing (Fallback)

Configure specific ports in config.yaml:
```yaml
ssh_routes:
  - listen_port: 2200
    destination_host: "github.com"
    destination_port: 22
  - listen_port: 2201
    destination_host: "gitlab.com"
    destination_port: 22
```

Then use different local ports:
```bash
ssh -p 2200 git@23.88.88.104  # Routes to github.com
ssh -p 2201 git@23.88.88.104  # Routes to gitlab.com
```

**Disadvantages:**
- ❌ Manual configuration per destination
- ❌ Ugly port numbers
- ❌ Doesn't work like other protocols

---

## Why /etc/hosts Doesn't Work for SSH

| Feature | HTTP/HTTPS | SSH |
|---------|------------|-----|
| **Hostname in protocol** | ✅ Host header / SNI | ❌ No hostname field |
| **Works with /etc/hosts** | ✅ Yes | ❌ No |
| **Needs iptables for transparent proxy** | ❌ No | ✅ Yes |
| **Can use ProxyCommand** | N/A | ✅ Yes |

**Summary:**
- **HTTP/HTTPS**: Hostname is IN the protocol → Works with /etc/hosts pointing to proxy
- **SSH**: No hostname in protocol → Needs iptables REDIRECT or ProxyCommand

---

## Current Fix Deployed

✅ **Loop Detection Added** (v1.0.1)

The proxy now detects when SO_ORIGINAL_DST points to itself and rejects the connection gracefully:

```
SSH loop detected - original destination is the proxy itself, trying fallback routing
No SSH routing available - enable transparent proxy (iptables REDIRECT) or configure ssh_routes
```

This prevents infinite loops when:
- SSH connects to proxy server directly
- /etc/hosts points to proxy without iptables

---

## Recommended Setup for You

Based on your setup, I recommend **iptables REDIRECT** (Solution 1):

```bash
# Remove github.com from /etc/hosts (or keep it for HTTP/HTTPS only)
# /etc/hosts should have:
23.88.88.104  ip.me

# Add iptables rule for automatic SSH routing
sudo iptables -t nat -A OUTPUT -p tcp --dport 22 ! -d 23.88.88.104 -j DNAT --to-destination 23.88.88.104:22

# Save rules
sudo apt-get install iptables-persistent
sudo netfilter-persistent save

# Test
ssh -T git@github.com
# Should work and show in proxy logs:
# "SSH auto-routing to original destination" original_dst="<real-github-ip>:22"
```

This way:
- HTTP/HTTPS continues to work with /etc/hosts (SNI-based routing)
- SSH works automatically with iptables (SO_ORIGINAL_DST-based routing)
- No manual configuration per host needed!

---

**See SSH_CLIENT_SETUP.md for complete setup instructions.**
