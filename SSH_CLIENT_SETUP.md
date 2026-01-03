# SSH Transparent Proxy - Client Setup Guide

SNIProxy now supports **automatic SSH proxying** just like HTTP/HTTPS!

## ⚠️ CRITICAL: This Setup is for YOUR LOCAL MACHINE (Client Side)

**DO NOT** try to SSH directly to the proxy server - that creates a loop!

The proxy intercepts SSH traffic on **your local machine** and routes it through the proxy server to real destinations like GitHub, GitLab, etc.

## Server Side (Already Done ✅)

Your proxy server at **23.88.88.104** is ready:
- ✅ Listening on port 22 for SSH traffic
- ✅ Server SSH moved to port 2222
- ✅ Loop detection enabled
- ✅ SO_ORIGINAL_DST support (Linux)

## Client Side Setup (You Need to Do This)

Choose **ONE** of these methods:

---

### Method 1: SSH ProxyCommand (Recommended)

Edit `~/.ssh/config` on your **LOCAL MACHINE**:

```bash
nano ~/.ssh/config
```

Add this configuration:

```
# Route all SSH through your proxy server
Host * !23.88.88.104
    ProxyCommand nc -X connect -x 23.88.88.104:22 %h %p

# Direct connection to proxy server's SSH
Host 23.88.88.104
    Port 2222
```

**Test it:**
```bash
ssh -T git@github.com
# Should say: "Hi <username>! You've successfully authenticated..."

git clone git@github.com:user/repo.git
# Should work through proxy!
```

---

### Method 2: iptables REDIRECT (Linux Only - Advanced)

On your **LOCAL MACHINE**, set up iptables to redirect all SSH:

```bash
# Redirect all SSH traffic (except to proxy server) to proxy
sudo iptables -t nat -A OUTPUT -p tcp --dport 22 ! -d 23.88.88.104 -j DNAT --to-destination 23.88.88.104:22

# Save rules (Ubuntu/Debian)
sudo apt-get install iptables-persistent
sudo netfilter-persistent save
```

**Test it:**
```bash
ssh -T git@github.com
git clone git@gitlab.com:user/repo.git
# All SSH traffic automatically proxied!
```

**Remove if needed:**
```bash
sudo iptables -t nat -D OUTPUT -p tcp --dport 22 ! -d 23.88.88.104 -j DNAT --to-destination 23.88.88.104:22
```

---

### Method 3: Per-Command Proxy (Simple Testing)

Test without permanent configuration:

```bash
# Single SSH command
ssh -o ProxyCommand="nc -X connect -x 23.88.88.104:22 %h %p" git@github.com

# Git clone
GIT_SSH_COMMAND='ssh -o ProxyCommand="nc -X connect -x 23.88.88.104:22 %h %p"' git clone git@github.com:user/repo.git
```

---

## How It Works

```
┌─────────────────────────────────────────────────────────┐
│              YOUR LOCAL MACHINE (Client)                 │
│                                                          │
│  1. You run: ssh git@github.com                         │
│  2. SSH config/iptables intercepts                      │
│  3. Connects to: 23.88.88.104:22 (proxy)               │
└────────────────────┬─────────────────────────────────────┘
                     │
                     ├─ [Proxy receives connection]
                     ├─ [Reads: destination = github.com:22]
                     ├─ [Connects to real github.com:22]
                     ├─ [Tunnels your SSH session]
                     │
                     ▼
            ┌─────────────────┐
            │  github.com:22  │
            │  gitlab.com:22  │
            │  any SSH server │
            └─────────────────┘
```

## Advantages

✅ **Works with ANY SSH destination** - GitHub, GitLab, your servers
✅ **No manual per-host configuration** - Just one setup
✅ **Transparent** - SSH clients work normally
✅ **Git works** - Clone, push, pull all automatic
✅ **Metrics tracked** - Prometheus metrics for all SSH connections
✅ **Loop protection** - Won't create infinite loops

## Troubleshooting

### "Connection reset" or "Connection refused"

**Problem:** You tried to SSH directly to the proxy server:
```bash
ssh root@23.88.88.104  # ❌ This will fail!
```

**Solution:** SSH to the proxy's management port:
```bash
ssh -p 2222 root@23.88.88.104  # ✅ Correct!
```

### "No route to host" or timeouts

1. **Check proxy is running:**
   ```bash
   ssh -p 2222 root@23.88.88.104 'systemctl status sniproxy'
   ```

2. **Check logs:**
   ```bash
   ssh -p 2222 root@23.88.88.104 'journalctl -u sniproxy -f'
   ```

3. **Test basic connectivity:**
   ```bash
   nc -zv 23.88.88.104 22  # Should connect
   nc -zv 23.88.88.104 2222  # Should connect
   ```

### Loop detection in logs

If you see:
```
SSH loop detected - original destination is the proxy itself
```

This means you're trying to SSH to the proxy server itself. Use port 2222 instead:
```bash
ssh -p 2222 root@23.88.88.104
```

## Testing Your Setup

Once configured, test with these commands on your **LOCAL MACHINE**:

```bash
# Test GitHub
ssh -T git@github.com
# Expected: "Hi <username>! You've successfully authenticated..."

# Test GitLab
ssh -T git@gitlab.com
# Expected: "Welcome to GitLab, @<username>!"

# Clone a repository
git clone git@github.com:user/repo.git

# Check proxy logs (from another terminal)
ssh -p 2222 root@23.88.88.104 'journalctl -u sniproxy -f'
# Should see: "SSH auto-routing to original destination"
```

## Security Note

The proxy **cannot decrypt** SSH traffic - it's end-to-end encrypted between your client and the destination server. The proxy just forwards the encrypted tunnel.

---

**Summary**: Set up SSH config or iptables on YOUR LOCAL MACHINE (not the server), then all SSH traffic automatically works through the proxy! 🚀
