# Fix "Too many open files" Error

## 🔴 Problem

You're seeing this error:
```
ERROR: Accept error: Too many open files (os error 24)
```

**Why it happens:**
- Each connection uses a file descriptor (Linux treats sockets as files)
- Default limit is 1024 file descriptors per process
- Your proxy is handling many connections and hitting this limit
- This causes the proxy to reject new connections

---

## ✅ Solution (2 Options)

### Option 1: Automatic Fix (Recommended)

**Upload and run the fix script:**

```bash
# 1. Upload fix script to server
scp fix_file_limits.sh user@23.88.88.105:/tmp/

# 2. SSH to server
ssh user@23.88.88.105

# 3. Run fix script
cd /tmp
sudo ./fix_file_limits.sh
```

**What it fixes:**
- ✅ Increases file descriptor limit to 1,048,576
- ✅ Updates systemd service
- ✅ Updates system-wide limits
- ✅ Updates kernel parameters
- ✅ Restarts service
- ✅ Verifies fix worked

---

### Option 2: Manual Fix

**Step 1: Update systemd service**

```bash
sudo nano /etc/systemd/system/sniproxy.service
```

Find the `[Service]` section and make sure these lines exist:

```ini
[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/sniproxy-server -c /etc/sniproxy/config.yaml
Restart=always
RestartSec=5

# ADD THESE LINES:
LimitNOFILE=1048576
LimitNPROC=65535
```

**Step 2: Update system limits**

```bash
sudo nano /etc/security/limits.conf
```

Add at the end:

```
# SNIProxy limits
root soft nofile 1048576
root hard nofile 1048576
* soft nofile 1048576
* hard nofile 1048576
```

**Step 3: Update kernel parameters**

```bash
sudo nano /etc/sysctl.conf
```

Add at the end:

```
# SNIProxy kernel tuning
fs.file-max = 2097152
fs.nr_open = 2097152
net.core.somaxconn = 65535
```

Apply changes:

```bash
sudo sysctl -p
```

**Step 4: Restart service**

```bash
sudo systemctl daemon-reload
sudo systemctl restart sniproxy
```

**Step 5: Verify**

```bash
sudo systemctl status sniproxy

# Check current limit
PID=$(systemctl show -p MainPID sniproxy | cut -d= -f2)
cat /proc/$PID/limits | grep "open files"

# Should show: Max open files  1048576  1048576  files
```

---

## 📊 Verification

After applying the fix, monitor the logs:

```bash
sudo journalctl -u sniproxy -f
```

**Before fix:**
```
ERROR: Accept error: Too many open files (os error 24)
ERROR: Accept error: Too many open files (os error 24)
ERROR: Accept error: Too many open files (os error 24)
```

**After fix:**
```
INFO: Proxy started, waiting for connections...
INFO: New connection from 192.168.1.100:54321
INFO: Extracted SNI from ClientHello: api.sohani.me
```

No more "Too many open files" errors! ✅

---

## 🔍 Understanding the Limits

### What is a file descriptor?
- In Linux, everything is a file (including network sockets)
- Each connection uses 1 file descriptor
- Default limit: 1024
- New limit: 1,048,576 (can handle 1 million connections!)

### What we changed:

1. **LimitNOFILE=1048576** (systemd)
   - Limits for the sniproxy service specifically
   - Overrides system defaults for this service

2. **/etc/security/limits.conf**
   - System-wide limits for all users/processes
   - Applies when logging in (PAM limits)

3. **/etc/sysctl.conf** (kernel parameters)
   - `fs.file-max` = Maximum files system-wide
   - `fs.nr_open` = Maximum files per process
   - `net.core.somaxconn` = Max connections in queue

---

## 💡 Why This Happened

Your proxy is popular and handling many connections! This is good, but the default Linux limits are conservative.

**Common scenarios:**
- Multiple clients connecting simultaneously
- Long-lived connections (WebSocket, HTTP/2)
- High traffic volume
- Connections not closing properly

**Solution:**
Increase limits to handle production load (1M+ connections)

---

## 🚨 Troubleshooting

### Still seeing errors after fix?

**Check if limits were applied:**

```bash
# Get sniproxy process ID
PID=$(systemctl show -p MainPID sniproxy | cut -d= -f2)

# Check limits
cat /proc/$PID/limits

# Look for "Max open files" - should show 1048576
```

**If limits are still low:**

```bash
# Make sure systemd was reloaded
sudo systemctl daemon-reload

# Make sure service was restarted (not just reloaded)
sudo systemctl restart sniproxy

# Check systemd service file
cat /etc/systemd/system/sniproxy.service | grep LimitNOFILE
# Should show: LimitNOFILE=1048576
```

**If service won't start:**

```bash
# Check for syntax errors
sudo systemctl status sniproxy -l

# Check logs
sudo journalctl -u sniproxy -n 50

# Test config
sudo /usr/local/bin/sniproxy-server -c /etc/sniproxy/config.yaml
# (Ctrl+C to stop)
```

---

## 📈 Monitoring File Descriptor Usage

**Check current usage:**

```bash
# Get process ID
PID=$(systemctl show -p MainPID sniproxy | cut -d= -f2)

# Count open files
ls -1 /proc/$PID/fd | wc -l

# Show limit
cat /proc/$PID/limits | grep "open files"
```

**Monitor in real-time:**

```bash
watch -n 1 "ls -1 /proc/\$(systemctl show -p MainPID sniproxy | cut -d= -f2)/fd | wc -l"
```

**Metrics available:**

```bash
curl http://localhost:9090/metrics | grep connections
# Shows: sniproxy_connections_active (current connections)
```

---

## ✅ Summary

**Problem:** Too many open files (limit: 1024)
**Solution:** Increase limit to 1,048,576
**How:** Run `sudo ./fix_file_limits.sh`
**Result:** Can handle 1 million simultaneous connections! 

---

## 📋 Quick Fix Commands

```bash
# Upload fix script
scp fix_file_limits.sh user@23.88.88.105:/tmp/

# SSH to server
ssh user@23.88.88.105

# Run fix
cd /tmp
sudo ./fix_file_limits.sh

# Verify (should show 1048576)
PID=$(systemctl show -p MainPID sniproxy | cut -d= -f2)
cat /proc/$PID/limits | grep "open files"

# Monitor (no more errors!)
sudo journalctl -u sniproxy -f
```

**Done! Your proxy can now handle massive traffic!** 🚀
