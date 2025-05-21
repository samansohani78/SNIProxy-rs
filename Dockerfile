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

# Install runtime dependencies
RUN apt update && \
    apt install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /usr/src/sniproxy/target/release/sniproxy-server /usr/local/bin/
COPY --from=builder /usr/src/sniproxy/config.yaml /etc/sniproxy/config.yaml

# Set proper permissions
RUN chown -R sniproxy:sniproxy /etc/sniproxy && \
    chmod +x /usr/local/bin/sniproxy-server

USER sniproxy

# Expose ports
EXPOSE 80 443 9000

# Run the proxy
CMD ["/usr/local/bin/sniproxy-server", "-c", "/etc/sniproxy/config.yaml"]
