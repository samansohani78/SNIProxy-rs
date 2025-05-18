#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Default values
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/sniproxy"
SERVICE_FILE="/etc/systemd/system/sniproxy.service"
USER="sniproxy"
GROUP="sniproxy"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Please run as root${NC}"
    exit 1
}

echo -e "${YELLOW}Installing SNIProxy...${NC}"

# Check for Rust toolchain
if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}Installing Rust toolchain...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Build release version
echo -e "${YELLOW}Building release version...${NC}"
cargo build --release

# Create user and group
if ! getent group "$GROUP" >/dev/null; then
    echo -e "${YELLOW}Creating group $GROUP...${NC}"
    groupadd -r "$GROUP"
fi

if ! getent passwd "$USER" >/dev/null; then
    echo -e "${YELLOW}Creating user $USER...${NC}"
    useradd -r -g "$GROUP" -s /bin/false "$USER"
fi

# Create directories and copy files
echo -e "${YELLOW}Installing binary and configuration...${NC}"
install -d -m 755 "$CONFIG_DIR"
install -m 755 "target/release/sniproxy-server" "$INSTALL_DIR/sniproxy-server"

# Copy config file if it doesn't exist
if [ ! -f "$CONFIG_DIR/config.yaml" ]; then
    install -m 644 "config.yaml" "$CONFIG_DIR/config.yaml"
else
    echo -e "${YELLOW}Config file already exists, skipping...${NC}"
fi

# Set permissions
chown -R "$USER:$GROUP" "$CONFIG_DIR"

# Install systemd service
echo -e "${YELLOW}Installing systemd service...${NC}"
install -m 644 "sniproxy.service" "$SERVICE_FILE"

# Configure system limits
echo -e "${YELLOW}Configuring system limits...${NC}"
if [ ! -f "/etc/sysctl.d/99-sniproxy.conf" ]; then
    cat > "/etc/sysctl.d/99-sniproxy.conf" << EOF
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
EOF
    sysctl -p /etc/sysctl.d/99-sniproxy.conf
fi

# Reload systemd and start service
echo -e "${YELLOW}Starting service...${NC}"
systemctl daemon-reload
systemctl enable sniproxy
systemctl restart sniproxy

# Check service status
if systemctl is-active --quiet sniproxy; then
    echo -e "${GREEN}SNIProxy has been successfully installed and started!${NC}"
    echo -e "\nService status:"
    systemctl status sniproxy --no-pager
    echo -e "\nTo view logs:"
    echo -e "  ${YELLOW}journalctl -u sniproxy -f${NC}"
    echo -e "\nConfig file location:"
    echo -e "  ${YELLOW}$CONFIG_DIR/config.yaml${NC}"
else
    echo -e "${RED}Failed to start SNIProxy service${NC}"
    echo -e "Check logs with: journalctl -u sniproxy"
    exit 1
fi

