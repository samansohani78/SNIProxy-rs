#!/bin/bash
set -e

echo "╔══════════════════════════════════════════════════════════════════════════╗"
echo "║              SNIProxy Installation Script                                ║"
echo "╚══════════════════════════════════════════════════════════════════════════╝"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo "❌ Please run as root or with sudo"
    echo "   Usage: sudo ./install.sh"
    exit 1
fi

echo "📦 Step 1: Installing binary..."
if [ ! -f "target/release/sniproxy-server" ]; then
    echo "❌ Binary not found. Please run 'cargo build --release' first"
    exit 1
fi

cp target/release/sniproxy-server /usr/local/bin/
chmod +x /usr/local/bin/sniproxy-server
echo "✅ Binary installed to /usr/local/bin/sniproxy-server"

echo ""
echo "📝 Step 2: Creating configuration..."
mkdir -p /etc/sniproxy

if [ ! -f "/etc/sniproxy/config.yaml" ]; then
    cat > /etc/sniproxy/config.yaml << 'YAML'
listen_addrs:
  - "0.0.0.0:80"
  - "0.0.0.0:443"

timeouts:
  connect: 10
  client_hello: 5
  idle: 300

metrics:
  enabled: true
  address: "127.0.0.1:9090"
YAML
    echo "✅ Created /etc/sniproxy/config.yaml"
else
    echo "ℹ️  Config already exists at /etc/sniproxy/config.yaml"
fi

echo ""
echo "🔧 Step 3: Creating systemd service..."
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

systemctl daemon-reload
echo "✅ Systemd service created"

echo ""
echo "🚀 Step 4: Enabling and starting service..."
systemctl enable sniproxy
systemctl start sniproxy

# Wait a moment for service to start
sleep 2

echo ""
echo "📊 Step 5: Checking status..."
if systemctl is-active --quiet sniproxy; then
    echo "✅ SNIProxy is running!"
    
    echo ""
    echo "Listening on:"
    netstat -tlnp | grep ':80\|:443' | grep sniproxy || echo "  (checking ports...)"
    
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "✅ INSTALLATION COMPLETE!"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "📋 Useful commands:"
    echo "   View logs:     sudo journalctl -u sniproxy -f"
    echo "   Check status:  sudo systemctl status sniproxy"
    echo "   Restart:       sudo systemctl restart sniproxy"
    echo "   Stop:          sudo systemctl stop sniproxy"
    echo "   View metrics:  curl http://localhost:9090/metrics"
    echo ""
    echo "📝 Configuration: /etc/sniproxy/config.yaml"
    echo "📖 Full guide:    cat DEPLOYMENT_GUIDE.md"
    echo ""
else
    echo "❌ Service failed to start"
    echo "   Check logs: sudo journalctl -u sniproxy -n 50"
    exit 1
fi
