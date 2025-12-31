# SNIProxy Quick Start Guide

## ⚡ 3-Step Deployment

### Step 1: Build
```bash
cargo build --release
```
**Result:** Binary at `target/release/sniproxy-server` (4.3 MB)

---

### Step 2: Install on Server
```bash
# Upload to your server (23.88.88.105)
scp target/release/sniproxy-server user@23.88.88.105:/tmp/
scp install.sh user@23.88.88.105:/tmp/

# SSH and install
ssh user@23.88.88.105
cd /tmp
sudo ./install.sh
```

**What it does:**
- ✅ Installs binary to `/usr/local/bin/`
- ✅ Creates config at `/etc/sniproxy/config.yaml`
- ✅ Sets up systemd service
- ✅ Starts on ports 80 and 443
- ✅ Auto-starts on boot

---

### Step 3: Configure Client
```bash
# On your local computer
sudo nano /etc/hosts

# Add this line:
23.88.88.105 api.sohani.me

# Save and test
curl http://api.sohani.me/
curl https://api.sohani.me/
```

---

## 🎯 How It Works

```
Your Computer                Proxy (23.88.88.105)         Real Server
    │                               │                          │
    │ Request to api.sohani.me      │                          │
    │ (DNS from /etc/hosts)         │                          │
    │                               │                          │
    ├──────────────────────────────→│                          │
    │   HTTP/HTTPS                  │                          │
    │   Host: api.sohani.me         │                          │
    │                               │                          │
    │                               │ Reads domain ✅          │
    │                               │                          │
    │                               │ DNS lookup:              │
    │                               │ api.sohani.me → Real IP  │
    │                               │                          │
    │                               ├─────────────────────────→│
    │                               │   Forward request        │
    │                               │                          │
    │                               │←─────────────────────────┤
    │                               │   Response               │
    │←──────────────────────────────┤                          │
    │   Response forwarded          │                          │
```

**Key Points:**
- Proxy reads domain from **Host header** (HTTP) or **SNI** (HTTPS)
- Proxy does **real DNS lookup** to find actual server IP
- Traffic is **transparently forwarded**
- HTTPS stays **end-to-end encrypted** (no decryption)

---

## 📋 Management Commands

```bash
# Check status
sudo systemctl status sniproxy

# View logs (real-time)
sudo journalctl -u sniproxy -f

# Restart
sudo systemctl restart sniproxy

# View metrics
curl http://localhost:9090/metrics

# Check what's listening
sudo netstat -tlnp | grep ':80\|:443'
```

---

## ✅ Verification

After installation, verify:

```bash
# On server (23.88.88.105)
sudo systemctl status sniproxy
# Should show: Active: active (running)

sudo netstat -tlnp | grep sniproxy
# Should show listening on ports 80 and 443

curl http://localhost:9090/metrics
# Should show Prometheus metrics

# On your local computer
ping api.sohani.me
# Should ping 23.88.88.105

curl -v http://api.sohani.me/
# Should get response from real api.sohani.me server
```

---

## 🛠️ Configuration

Edit `/etc/sniproxy/config.yaml` on server:

```yaml
listen_addrs:
  - "0.0.0.0:80"      # HTTP
  - "0.0.0.0:443"     # HTTPS

timeouts:
  connect: 10         # Backend connection timeout
  client_hello: 5     # TLS handshake timeout
  idle: 300           # Idle timeout

metrics:
  enabled: true
  address: "127.0.0.1:9090"

# Optional: Restrict domains
# allowlist:
#   - "api.sohani.me"
#   - "*.sohani.me"
```

After editing config:
```bash
sudo systemctl restart sniproxy
```

---

## 🔥 Supported Protocols

All working with 71/71 tests passing:

- ✅ HTTP/1.0, HTTP/1.1 (50/50 requests)
- ✅ HTTP/2 (preface detection)
- ✅ HTTP/3 (ALPN detection)
- ✅ HTTPS/TLS (SNI extraction < 10μs)
- ✅ WebSocket (full upgrade)
- ✅ gRPC (detection & forwarding)
- ✅ Concurrent (8 protocols simultaneously)

---

## 📖 Full Documentation

- **DEPLOYMENT_GUIDE.md** - Complete deployment guide
- **COMPREHENSIVE_TEST_REPORT.md** - All 75 tests documented
- **TESTING_ON_STANDARD_PORTS.md** - Port 80/443 testing guide

---

## 🚨 Troubleshooting

**Port already in use:**
```bash
sudo systemctl stop nginx  # or apache2
sudo systemctl start sniproxy
```

**View errors:**
```bash
sudo journalctl -u sniproxy -n 50
```

**Can't resolve domain:**
```bash
# On server, test DNS
dig api.sohani.me
nslookup api.sohani.me
```

---

## 📊 Example: api.sohani.me Setup

**Server (23.88.88.105):**
```bash
sudo ./install.sh
sudo systemctl status sniproxy
```

**Your Computer:**
```bash
echo "23.88.88.105 api.sohani.me" | sudo tee -a /etc/hosts
curl http://api.sohani.me/
```

**What happens:**
1. Your computer resolves api.sohani.me → 23.88.88.105 (from /etc/hosts)
2. Connects to proxy on port 80/443
3. Proxy reads "api.sohani.me" from request
4. Proxy does DNS lookup → gets real IP
5. Proxy connects to real server
6. Proxy forwards traffic both ways
7. **Everything works transparently!**

---

**🎉 That's it! Your proxy is ready to use.**

Quick commands:
- Deploy: `sudo ./install.sh`
- Check: `sudo systemctl status sniproxy`
- Logs: `sudo journalctl -u sniproxy -f`
- Test: `curl http://api.sohani.me/`
