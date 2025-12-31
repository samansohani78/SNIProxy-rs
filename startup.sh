#!/bin/sh
set -e

echo "Starting SNIProxy startup script with enhanced debugging..."

# Display system information
echo "=== System Information ==="
uname -a
echo "=== Docker Container ID ==="
cat /proc/self/cgroup | grep -o -E '[0-9a-f]{64}' | head -n 1 || echo "Could not determine container ID"
echo "=== Process Information ==="
ps aux || echo "ps command failed"

# Function to check if a port is available using ss
check_port_available() {
    local port=$1
    echo "Checking if port $port is available..."
    
    # Use ss to check if the port is in use
    if ss -tuln | grep -q ":$port "; then
        echo "WARNING: Port $port appears to be in use!"
        # Show what process is using this port
        echo "Process using port $port:"
        lsof -i ":$port" || echo "Could not determine process using port $port"
        return 1
    else
        echo "Port $port is available"
        return 0
    fi
}

# Display network information
echo "=== Current Network Configuration ==="
ip addr || echo "ip addr command failed"
echo "=== Current Listening Ports ==="
ss -tuln || echo "ss command failed"

# Check our required ports
echo "=== Checking SNIProxy Ports ==="
check_port_available 28080 || echo "Warning: HTTP port may not be available"
check_port_available 28443 || echo "Warning: HTTPS port may not be available"
check_port_available 29090 || echo "Warning: Metrics port may not be available"

# Note about SO_REUSEADDR
echo "=== Socket Options ==="
echo "Note: SNIProxy should use SO_REUSEADDR to allow port reuse"
echo "This is handled internally by the Rust networking stack"

# Start SNIProxy with exec to ensure signal handling works properly
echo "=== Starting SNIProxy Server ==="
echo "Command: /usr/local/bin/sniproxy-server -c /etc/sniproxy/config.yaml"
exec /usr/local/bin/sniproxy-server -c /etc/sniproxy/config.yaml

