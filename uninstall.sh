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
SYSCTL_FILE="/etc/sysctl.d/99-sniproxy.conf"
USER="sniproxy"
GROUP="sniproxy"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Please run as root${NC}"
    exit 1
}

echo -e "${YELLOW}Uninstalling SNIProxy...${NC}"

# Stop and disable service
echo -e "${YELLOW}Stopping service...${NC}"
if systemctl is-active --quiet sniproxy; then
    systemctl stop sniproxy
fi
if systemctl is-enabled --quiet sniproxy; then
    systemctl disable sniproxy
fi

# Backup configuration if exists
if [ -f "$CONFIG_DIR/config.yaml" ]; then
    BACKUP_FILE="/tmp/sniproxy-config-$(date +%Y%m%d-%H%M%S).yaml"
    echo -e "${YELLOW}Backing up configuration to $BACKUP_FILE...${NC}"
    cp "$CONFIG_DIR/config.yaml" "$BACKUP_FILE"
fi

# Remove files
echo -e "${YELLOW}Removing files...${NC}"
rm -f "$INSTALL_DIR/sniproxy-server"
rm -f "$SERVICE_FILE"
rm -f "$SYSCTL_FILE"
rm -rf "$CONFIG_DIR"

# Remove user and group
echo -e "${YELLOW}Removing user and group...${NC}"
if getent passwd "$USER" >/dev/null; then
    userdel "$USER"
fi
if getent group "$GROUP" >/dev/null; then
    groupdel "$GROUP"
fi

# Reload systemd
echo -e "${YELLOW}Reloading systemd...${NC}"
systemctl daemon-reload

# Apply system limits (restore defaults)
if [ -f "$SYSCTL_FILE" ]; then
    echo -e "${YELLOW}Restoring system limits...${NC}"
    sysctl --system
fi

echo -e "${GREEN}SNIProxy has been successfully uninstalled!${NC}"
if [ -f "$BACKUP_FILE" ]; then
    echo -e "\nConfiguration backup saved to: ${YELLOW}$BACKUP_FILE${NC}"
fi

echo -e "\nTo remove all build artifacts, you may also want to run:"
echo -e "${YELLOW}rm -rf ~/.cargo/registry/src/*/sniproxy-*${NC}"
echo -e "${YELLOW}rm -rf ./target${NC}"

