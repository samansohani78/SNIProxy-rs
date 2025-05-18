#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Starting SNIProxy build and test process...${NC}"

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo is not installed. Please install Rust and cargo first.${NC}"
    exit 1
}

# Clean previous builds
echo -e "${YELLOW}Cleaning previous builds...${NC}"
cargo clean

# Format code
echo -e "${YELLOW}Formatting code...${NC}"
cargo fmt --all -- --check || {
    echo -e "${RED}Code formatting check failed. Running cargo fmt to fix...${NC}"
    cargo fmt --all
}

# Run clippy
echo -e "${YELLOW}Running clippy for code analysis...${NC}"
cargo clippy -- -D warnings

# Build in debug mode
echo -e "${YELLOW}Building in debug mode...${NC}"
cargo build

# Run tests
echo -e "${YELLOW}Running tests...${NC}"
cargo test

# Build in release mode
echo -e "${YELLOW}Building in release mode...${NC}"
cargo build --release

echo -e "${GREEN}Build process completed successfully!${NC}"
echo -e "${YELLOW}To run the proxy:${NC}"
echo -e "Debug mode:   ${GREEN}sudo ./target/debug/sniproxy-server -c config.yaml${NC}"
echo -e "Release mode: ${GREEN}sudo ./target/release/sniproxy-server -c config.yaml${NC}"
