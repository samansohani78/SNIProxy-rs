# Build stage
FROM rust:1.87-slim-bookworm AS builder

WORKDIR /usr/src/sniproxy
COPY . .

# Install build dependencies
RUN apt update && \
    apt install -y pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Build application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Create non-root user
RUN useradd -m -U -u 1000 -s /bin/false sniproxy

# Install runtime dependencies and enhanced networking/debugging tools
RUN apt update && \
    apt install -y ca-certificates procps net-tools lsof wget curl \
                   iproute2 iputils-ping netcat-openbsd tcpdump \
                   dnsutils psmisc && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /usr/src/sniproxy/target/release/sniproxy-server /usr/local/bin/
COPY --from=builder /usr/src/sniproxy/config.yaml /etc/sniproxy/config.yaml

# Copy our startup script
COPY startup.sh /usr/local/bin/

# Add port-check script
RUN echo '#!/bin/sh\n\
# Function to check if a port is in use\n\
check_port() {\n\
    local port=$1\n\
    local timeout=10\n\
    local start_time=$(date +%s)\n\
    local current_time\n\
\n\
    echo "Checking port $port..."\n\
    while true; do\n\
        # Try binding to the port with a quick timeout\n\
        if nc -z -w 1 localhost $port 2>/dev/null; then\n\
            echo "WARNING: Port $port is in use"\n\
            # Get process using the port\n\
            local pid=$(lsof -i :$port -t 2>/dev/null)\n\
            if [ -n "$pid" ]; then\n\
                echo "Process using port $port: $(ps -p $pid -o comm=)"\n\
                if [ "$2" = "force" ]; then\n\
                    echo "Attempting to kill process $pid..."\n\
                    kill -9 $pid 2>/dev/null || true\n\
                    sleep 1\n\
                fi\n\
            else\n\
                echo "Cannot identify process using port $port"\n\
                # Try to force release the port with netstat/ss\n\
                if [ "$2" = "force" ]; then\n\
                    echo "Attempting to close all TIME_WAIT sockets on port $port..."\n\
                    # Force close TIME_WAIT sockets\n\
                    echo 1 > /proc/sys/net/ipv4/tcp_tw_recycle 2>/dev/null || true\n\
                    sleep 1\n\
                fi\n\
            fi\n\
        else\n\
            echo "Port $port is available"\n\
            return 0\n\
        fi\n\
\n\
        # Check if we\'ve exceeded the timeout\n\
        current_time=$(date +%s)\n\
        if [ $((current_time - start_time)) -ge $timeout ]; then\n\
            echo "Timeout waiting for port $port to become available"\n\
            return 1\n\
        fi\n\
\n\
        sleep 1\n\
    done\n\
}\n\
\n\
# Trap for cleanup on exit\n\
cleanup() {\n\
    echo "Container stopping, performing cleanup..."\n\
    # Add any cleanup tasks here\n\
    exit 0\n\
}\n\
\n\
trap cleanup SIGTERM SIGINT\n\
' > /usr/local/bin/port-check.sh

# Add enhanced debugging script
RUN echo '#!/bin/sh\n\
set -e\n\
\n\
# Source the port check functions\n\
. /usr/local/bin/port-check.sh\n\
\n\
echo "Running enhanced debugging mode..."\n\
echo "=== System Information ==="\n\
uname -a\n\
echo "=== Container Network Config ==="\n\
ip addr || echo "ip addr command failed"\n\
echo "=== DNS Configuration ==="\n\
cat /etc/resolv.conf || echo "No DNS config found"\n\
echo "=== Active Internet Connections ==="\n\
ss -tuln || echo "ss command failed"\n\
netstat -tuln || echo "netstat command failed"\n\
echo "=== Process List ==="\n\
ps aux || echo "ps failed"\n\
\n\
# Check each required port and attempt to force release if needed\n\
echo "=== Port Availability Check ==="\n\
check_port 38080 "force"\n\
check_port 38443 "force"\n\
check_port 39090 "force"\n\
\n\
echo "=== Starting SNIProxy with verbose logging ==="\n\
exec /usr/local/bin/sniproxy-server -c /etc/sniproxy/config.yaml\n\
' > /usr/local/bin/debug-startup.sh

# Set proper permissions
RUN chown -R sniproxy:sniproxy /etc/sniproxy && \
    chmod +x /usr/local/bin/sniproxy-server && \
    chmod +x /usr/local/bin/startup.sh && \
    chmod +x /usr/local/bin/port-check.sh && \
    chmod +x /usr/local/bin/debug-startup.sh && \
    chown sniproxy:sniproxy /usr/local/bin/startup.sh && \
    chown sniproxy:sniproxy /usr/local/bin/port-check.sh && \
    chown sniproxy:sniproxy /usr/local/bin/debug-startup.sh

# We need to run as root for some network operations and process management
# Will switch to sniproxy user in the startup script when appropriate

# Expose our actual ports
EXPOSE 38080 38443 39090

# Use the enhanced debug startup script
CMD ["/usr/local/bin/debug-startup.sh"]
