#!/bin/bash
# Quick fix for "Too many open files" error
# Run with: sudo bash quick_fix.sh

set -e

echo "🔧 Fixing 'Too many open files' error..."

# Update systemd service
cat > /etc/systemd/system/sniproxy.service << 'EOF'
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

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/log

# CRITICAL: File descriptor limits
LimitNOFILE=1048576
LimitNPROC=65535

[Install]
WantedBy=multi-user.target
EOF

echo "✅ Updated systemd service"

# Update system limits
if ! grep -q "SNIProxy limits" /etc/security/limits.conf; then
    cat >> /etc/security/limits.conf << 'EOF'

# SNIProxy limits
root soft nofile 1048576
root hard nofile 1048576
* soft nofile 1048576
* hard nofile 1048576
EOF
    echo "✅ Updated /etc/security/limits.conf"
else
    echo "ℹ️  Limits already in /etc/security/limits.conf"
fi

# Update kernel parameters
if ! grep -q "fs.file-max" /etc/sysctl.conf; then
    cat >> /etc/sysctl.conf << 'EOF'

# SNIProxy kernel tuning
fs.file-max = 2097152
fs.nr_open = 2097152
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 8192
EOF
    sysctl -p > /dev/null 2>&1
    echo "✅ Updated kernel parameters"
else
    echo "ℹ️  Kernel parameters already tuned"
fi

# Reload and restart
echo "🔄 Restarting service..."
systemctl daemon-reload
systemctl restart sniproxy

sleep 2

# Verify
PID=$(systemctl show -p MainPID sniproxy | cut -d= -f2)
if [ "$PID" != "0" ]; then
    LIMIT=$(cat /proc/$PID/limits | grep "open files" | awk '{print $4}')
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "✅ FIX COMPLETE!"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Current limit: $LIMIT file descriptors"
    echo ""
    echo "Monitor logs: sudo journalctl -u sniproxy -f"
else
    echo "❌ Service failed to start"
    echo "Check logs: sudo journalctl -u sniproxy -n 50"
fi
