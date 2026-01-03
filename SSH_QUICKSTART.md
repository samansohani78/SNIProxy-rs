# SSH Transparent Proxy - Quick Start

SNIProxy now supports **automatic SSH proxying** just like HTTP/HTTPS!

## How It Works

- **HTTP/HTTPS**: Proxy reads Host header or SNI → Forwards to destination ✅
- **SSH**: Proxy reads original destination from iptables → Forwards to destination ✅

**No manual configuration per destination needed!**

## Setup (Linux)

### 1. Server SSH Port Change

The server's SSH has been moved to port 2222 to free up port 22 for the proxy:

```bash
# Connect to server SSH on port 2222
ssh -p 2222 user@your-server-ip
```

### 2. SNIProxy Listens on Port 22

Config is already updated:
```yaml
listen_addrs:
  - "0.0.0.0:22"      # SSH transparent proxy
```

### 3. Client Setup - iptables Redirect

On your **local machine** (client), redirect SSH traffic to the proxy:

```bash
# Redirect SSH to proxy (automatic destination detection)
sudo iptables -t nat -A OUTPUT -p tcp --dport 22 -j REDIRECT --to-ports 22

# Make permanent (Ubuntu/Debian)
sudo apt-get install iptables-persistent
sudo netfilter-persistent save
```

### 4. Test It!

```bash
# SSH to GitHub - automatically proxied!
ssh -T git@github.com

# SSH to GitLab - automatically proxied!
ssh -T git@gitlab.com

# Git clone - automatically proxied!
git clone git@github.com:username/repo.git

# SSH to ANY server - automatically proxied!
ssh user@anyserver.com
```

## How Automatic Detection Works

1. You run: `ssh git@github.com`
2. System tries to connect to github.com:22
3. iptables redirects to localhost:22 (proxy)
4. **Proxy reads SO_ORIGINAL_DST** → sees destination was github.com:22
5. Proxy connects to github.com:22 and tunnels your connection
6. ✅ Everything works transparently!

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Your Local Machine                        │
│                                                              │
│  ssh git@github.com  →  iptables REDIRECT  →  SNIProxy:22  │
│                          (captures dest)       (port 22)     │
└────────────────────────────┬─────────────────────────────────┘
                             │
                             │ [Reads SO_ORIGINAL_DST = github.com:22]
                             │ [Connects to real github.com:22]
                             │
                             ↓
                    ┌────────────────┐
                    │  github.com:22 │
                    │  gitlab.com:22 │
                    │  any-ssh-server│
                    └────────────────┘
```

## Advantages

✅ **Works with ANY SSH destination** - GitHub, GitLab, your own servers, etc.
✅ **No manual configuration** - Just iptables rule once
✅ **Transparent** - SSH clients work normally
✅ **Git operations work** - Clone, push, pull all work
✅ **Metrics tracked** - Prometheus metrics for all SSH connections

## Remove Redirect (if needed)

```bash
# Remove the iptables rule
sudo iptables -t nat -D OUTPUT -p tcp --dport 22 -j REDIRECT --to-ports 22

# Or flush all NAT OUTPUT rules (be careful!)
sudo iptables -t nat -F OUTPUT
```

## Troubleshooting

### Can't connect to server SSH
If you need to connect directly to the server:
```bash
# Use port 2222 for server SSH
ssh -p 2222 user@server-ip
```

### SSH hangs or times out
Check if proxy is running:
```bash
systemctl status sniproxy
sudo ss -tlnp | grep :22
```

### Want to bypass proxy for specific host
```bash
# Temporarily disable proxy for one connection
sudo iptables -t nat -D OUTPUT -p tcp --dport 22 -j REDIRECT --to-ports 22
ssh user@host
sudo iptables -t nat -A OUTPUT -p tcp --dport 22 -j REDIRECT --to-ports 22
```

## Security Note

The proxy **cannot decrypt** SSH traffic - it's end-to-end encrypted between your client and the destination server. The proxy just forwards the encrypted tunnel.

---

**Summary**: SSH now works exactly like HTTP/HTTPS - automatic routing without per-destination configuration! 🚀
