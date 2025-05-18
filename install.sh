#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo -e "${RED}Please run as root${NC}"
    exit 1
fi

echo -e "${YELLOW}Installing SNIProxy...${NC}"

# Install system dependencies
echo -e "${YELLOW}Installing dependencies...${NC}"
apt-get update
apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    curl \
    ca-certificates

# Install Rust if not present
if ! command -v rustc &> /dev/null; then
    echo -e "${YELLOW}Installing Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Create sniproxy user and group if they don't exist
if ! getent group sniproxy >/dev/null; then
    echo -e "${YELLOW}Creating sniproxy group...${NC}"
    groupadd -r sniproxy
fi

if ! getent passwd sniproxy >/dev/null; then
    echo -e "${YELLOW}Creating sniproxy user...${NC}"
    useradd -r -g sniproxy -s /bin/false sniproxy
fi

# Create directories
echo -e "${YELLOW}Creating directories...${NC}"
mkdir -p /etc/sniproxy
mkdir -p /var/log/sniproxy

# Build the project
echo -e "${YELLOW}Building SNIProxy...${NC}"
cargo build --release

# Install binary and configuration
echo -e "${YELLOW}Installing files...${NC}"
install -m 755 target/release/sniproxy-server /usr/local/bin/
install -m 644 config.yaml /etc/sniproxy/
install -m 644 sniproxy.service /etc/systemd/system/

# Set permissions
echo -e "${YELLOW}Setting permissions...${NC}"
chown -R sniproxy:sniproxy /etc/sniproxy
chown -R sniproxy:sniproxy /var/log/sniproxy
chown root:root /usr/local/bin/sniproxy-server
chmod 755 /usr/local/bin/sniproxy-server

# Configure system limits
echo -e "${YELLOW}Configuring system limits...${NC}"
cat > /etc/sysctl.d/99-sniproxy.conf << EOF
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
EOF
sysctl --system

# Start service
echo -e "${YELLOW}Starting service...${NC}"
systemctl daemon-reload
systemctl enable sniproxy
systemctl restart sniproxy

# Verify installation
echo -e "${YELLOW}Verifying installation...${NC}"
if systemctl is-active --quiet sniproxy; then
    echo -e "${GREEN}SNIProxy is installed and running!${NC}"
    echo -e "\nService status:"
    systemctl status sniproxy --no-pager
    echo -e "\nListening ports:"
    ss -tlnp | grep sniproxy-server
    echo -e "\nTo view logs:"
    echo -e "  ${YELLOW}journalctl -u sniproxy -f${NC}"
    echo -e "\nConfig file location:"
    echo -e "  ${YELLOW}/etc/sniproxy/config.yaml${NC}"
else
    echo -e "${RED}Installation failed. Please check the logs:${NC}"
    journalctl -u sniproxy --no-pager
    exit 1
fi
