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
    ca-certificates \
    net-tools

# Install Rust if not present
if ! command -v rustc &> /dev/null; then
    echo -e "${YELLOW}Installing Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Create SNIProxy-rs user and group if they don't exist
if ! getent group SNIProxy-rs >/dev/null; then
    echo -e "${YELLOW}Creating SNIProxy-rs group...${NC}"
    groupadd -r SNIProxy-rs
fi

if ! getent passwd SNIProxy-rs >/dev/null; then
    echo -e "${YELLOW}Creating SNIProxy-rs user...${NC}"
    useradd -r -g SNIProxy-rs -s /bin/false SNIProxy-rs
fi

# Create directories
echo -e "${YELLOW}Creating directories...${NC}"
mkdir -p /etc/SNIProxy-rs
mkdir -p /var/log/SNIProxy-rs

# Create default config if it doesn't exist
if [ ! -f config.yaml ]; then
    echo -e "${YELLOW}Creating default config.yaml...${NC}"
    cat > config.yaml << 'END'
timeouts:
  connect: 10
  client_hello: 10
  idle: 300

listen_addrs:
  - "0.0.0.0:80"
  - "0.0.0.0:443"

metrics:
  enabled: true
  address: "127.0.0.1:9000"

allowlist: ["*"]
END
fi

# Build the project
echo -e "${YELLOW}Building SNIProxy...${NC}"
cargo build --release

# Install binary and configuration
echo -e "${YELLOW}Installing files...${NC}"
install -m 755 target/release/SNIProxy-rs-server /usr/local/bin/
install -m 644 config.yaml /etc/SNIProxy-rs/
install -m 644 SNIProxy-rs.service /etc/systemd/system/

# Set permissions
echo -e "${YELLOW}Setting permissions...${NC}"
chown -R SNIProxy-rs:SNIProxy-rs /etc/SNIProxy-rs
chown -R SNIProxy-rs:SNIProxy-rs /var/log/SNIProxy-rs
chown root:root /usr/local/bin/SNIProxy-rs-server
chmod 755 /usr/local/bin/SNIProxy-rs-server

# Configure system limits
echo -e "${YELLOW}Configuring system limits...${NC}"
cat > /etc/sysctl.d/99-SNIProxy-rs.conf << EOF
net.core.somaxconn = 65535
net.ipv4.tcp_max_syn_backlog = 65535
