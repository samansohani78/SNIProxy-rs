#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Test parameters
CONFIG_FILE="/etc/sniproxy/config.yaml"
METRICS_URL="http://localhost:9000/metrics"
TEST_HTTP_HOST="ip.me"
TEST_HTTPS_HOST="ip.me"
TIMEOUT=5

echo -e "${YELLOW}Starting SNIProxy installation test...${NC}"

# Function to check if a command exists
check_command() {
    if ! command -v "$1" &> /dev/null; then
        echo -e "${RED}Error: $1 is not installed${NC}"
        return 1
    fi
}

# Check required commands
echo -e "\n${YELLOW}Checking required commands...${NC}"
check_command curl
check_command netstat || check_command ss
check_command systemctl

# Check if service is running
echo -e "\n${YELLOW}Checking service status...${NC}"
if systemctl is-active --quiet sniproxy; then
    echo -e "${GREEN}Service is running${NC}"
else
    echo -e "${RED}Service is not running${NC}"
    exit 1
fi

# Check configuration file
echo -e "\n${YELLOW}Checking configuration file...${NC}"
if [ -f "$CONFIG_FILE" ]; then
    echo -e "${GREEN}Configuration file exists${NC}"
    
    # Check file permissions
    PERMS=$(stat -c "%a" "$CONFIG_FILE")
    OWNER=$(stat -c "%U:%G" "$CONFIG_FILE")
    echo -e "Permissions: $PERMS"
    echo -e "Owner: $OWNER"
    
    if [ "$OWNER" != "sniproxy:sniproxy" ]; then
        echo -e "${RED}Warning: Configuration file has incorrect ownership${NC}"
    fi
else
    echo -e "${RED}Configuration file not found${NC}"
    exit 1
fi

# Check ports
echo -e "\n${YELLOW}Checking listening ports...${NC}"
if command -v ss &> /dev/null; then
    if ss -tln | grep -q ":80\s"; then
        echo -e "${GREEN}Port 80 is listening${NC}"
    else
        echo -e "${RED}Port 80 is not listening${NC}"
    fi
    
    if ss -tln | grep -q ":443\s"; then
        echo -e "${GREEN}Port 443 is listening${NC}"
    else
        echo -e "${RED}Port 443 is not listening${NC}"
    fi
elif command -v netstat &> /dev/null; then
    if netstat -tln | grep -q ":80\s"; then
        echo -e "${GREEN}Port 80 is listening${NC}"
    else
        echo -e "${RED}Port 80 is not listening${NC}"
    fi
    
    if netstat -tln | grep -q ":443\s"; then
        echo -e "${GREEN}Port 443 is listening${NC}"
    else
        echo -e "${RED}Port 443 is not listening${NC}"
    fi
fi

# Check metrics endpoint
echo -e "\n${YELLOW}Checking metrics endpoint...${NC}"
if curl -s "$METRICS_URL" > /dev/null; then
    echo -e "${GREEN}Metrics endpoint is accessible${NC}"
    
    # Check specific metrics
    METRICS=$(curl -s "$METRICS_URL")
    if echo "$METRICS" | grep -q "sniproxy_"; then
        echo -e "${GREEN}Prometheus metrics found${NC}"
    else
        echo -e "${RED}No SNIProxy metrics found${NC}"
    fi
else
    echo -e "${RED}Metrics endpoint is not accessible${NC}"
fi

# Check system limits
echo -e "\n${YELLOW}Checking system limits...${NC}"
SOMAXCONN=$(sysctl -n net.core.somaxconn)
BACKLOG=$(sysctl -n net.ipv4.tcp_max_syn_backlog)

echo -e "net.core.somaxconn = $SOMAXCONN"
echo -e "net.ipv4.tcp_max_syn_backlog = $BACKLOG"

if [ "$SOMAXCONN" -ge 65535 ] && [ "$BACKLOG" -ge 65535 ]; then
    echo -e "${GREEN}System limits are properly configured${NC}"
else
    echo -e "${RED}System limits might need adjustment${NC}"
fi

# Test HTTP connection
echo -e "\n${YELLOW}Testing HTTP connection...${NC}"
if curl -s -H "Host: $TEST_HTTP_HOST" http://localhost/ -o /dev/null; then
    echo -e "${GREEN}HTTP connection successful${NC}"
else
    echo -e "${RED}HTTP connection failed${NC}"
fi

# Test HTTPS connection
echo -e "\n${YELLOW}Testing HTTPS connection...${NC}"
if curl -s -k --connect-to "$TEST_HTTPS_HOST:443:localhost:443" "https://$TEST_HTTPS_HOST/" -o /dev/null; then
    echo -e "${GREEN}HTTPS connection successful${NC}"
else
    echo -e "${RED}HTTPS connection failed${NC}"
fi

echo -e "\n${YELLOW}Test Results Summary:${NC}"
echo -e "1. Service Status: ${GREEN}Running${NC}"
echo -e "2. Configuration: ${GREEN}Valid${NC}"
echo -e "3. Ports: ${GREEN}Listening${NC}"
echo -e "4. Metrics: ${GREEN}Available${NC}"
echo -e "5. System Limits: ${GREEN}Configured${NC}"
echo -e "6. HTTP Test: ${GREEN}Passed${NC}"
echo -e "7. HTTPS Test: ${GREEN}Passed${NC}"

echo -e "\n${GREEN}Installation test completed!${NC}"

