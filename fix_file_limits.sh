#!/bin/bash
set -e

echo "╔══════════════════════════════════════════════════════════════════════════╗"
echo "║          Fixing 'Too many open files' Error                             ║"
echo "╚══════════════════════════════════════════════════════════════════════════╝"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo "❌ Please run as root or with sudo"
    echo "   Usage: sudo ./fix_file_limits.sh"
    exit 1
fi

echo "🔧 Step 1: Updating systemd service file..."

# Update systemd service with correct limits
cat > /etc/systemd/system/sniproxy.service << 'SERVICE'
[Unit]
Description=SNIProxy - High-performance SNI/Host-based proxy
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/sniproxy-server -c /etc/sniproxy/config.yaml
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal
Environment="RUST_LOG=sniproxy=info"

# Security settings
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/log

# IMPORTANT: File descriptor limits
LimitNOFILE=1048576
LimitNPROC=65535

[Install]
WantedBy=multi-user.target
SERVICE

echo "✅ Updated systemd service with LimitNOFILE=1048576"

echo ""
echo "🔧 Step 2: Updating system-wide limits..."

# Update /etc/security/limits.conf
if ! grep -q "sniproxy limits" /etc/security/limits.conf; then
    cat >> /etc/security/limits.conf << 'LIMITS'

# SNIProxy limits
root soft nofile 1048576
root hard nofile 1048576
* soft nofile 1048576
* hard nofile 1048576
LIMITS
    echo "✅ Updated /etc/security/limits.conf"
else
    echo "ℹ️  Limits already set in /etc/security/limits.conf"
fi

echo ""
echo "🔧 Step 3: Updating kernel parameters..."

# Update sysctl settings
if ! grep -q "fs.file-max" /etc/sysctl.conf; then
    cat >> /etc/sysctl.conf << 'SYSCTL'

# SNIProxy kernel tuning
fs.file-max = 2097152
fs.nr_open = 2097152
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 8192
net.ipv4.ip_local_port_range = 1024 65535
SYSCTL
    sysctl -p > /dev/null 2>&1
    echo "✅ Updated kernel parameters"
else
    echo "ℹ️  Kernel parameters already tuned"
fi

echo ""
echo "🔧 Step 4: Reloading systemd and restarting service..."

systemctl daemon-reload
systemctl restart sniproxy

# Wait for service to start
sleep 2

echo ""
echo "🔍 Step 5: Verifying fix..."

if systemctl is-active --quiet sniproxy; then
    echo "✅ SNIProxy is running!"
    
    # Check current limits
    PID=$(systemctl show -p MainPID sniproxy | cut -d= -f2)
    if [ "$PID" != "0" ]; then
        CURRENT_LIMIT=$(cat /proc/$PID/limits | grep "open files" | awk '{print $4}')
        echo "✅ Current file descriptor limit: $CURRENT_LIMIT"
        
        if [ "$CURRENT_LIMIT" -ge 1048576 ]; then
            echo "✅ Limit successfully increased!"
        else
            echo "⚠️  Limit is $CURRENT_LIMIT (should be 1048576)"
        fi
    fi
    
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "✅ FIX COMPLETE - Error should be resolved!"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "📋 Monitoring:"
    echo "   View logs:  sudo journalctl -u sniproxy -f"
    echo "   No more 'Too many open files' errors should appear!"
    echo ""
else
    echo "❌ Service failed to start"
    echo "   Check logs: sudo journalctl -u sniproxy -n 50"
    exit 1
fi
