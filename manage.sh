#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Default config file location
CONFIG_FILE="config.yaml"

function print_help() {
    echo -e "Usage: $0 [command] [options]"
    echo -e "\nCommands:"
    echo -e "  build              Build the project"
    echo -e "  test               Run tests"
    echo -e "  run                Run the proxy directly"
    echo -e "  docker-build       Build Docker image"
    echo -e "  docker-run         Run using Docker Compose"
    echo -e "  docker-stop        Stop Docker containers"
    echo -e "  clean              Clean build artifacts"
    echo -e "  check              Run format and lint checks"
    echo -e "\nOptions:"
    echo -e "  -c, --config       Specify config file (default: config.yaml)"
    echo -e "  -r, --release      Use release mode"
    echo -e "  -h, --help         Show this help message"
}

function check_requirements() {
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}Error: cargo is not installed. Please install Rust and cargo first.${NC}"
        exit 1
    fi
}

function build() {
    echo -e "${YELLOW}Building project...${NC}"
    if [ "$1" == "release" ]; then
        cargo build --release
    else
        cargo build
    fi
}

function run_tests() {
    echo -e "${YELLOW}Running tests...${NC}"
    cargo test
    
    echo -e "${YELLOW}Running clippy...${NC}"
    cargo clippy -- -D warnings
    
    echo -e "${YELLOW}Checking formatting...${NC}"
    cargo fmt -- --check
}

function run_proxy() {
    local mode=$1
    local config=$2
    
    if [ ! -f "$config" ]; then
        echo -e "${RED}Error: Config file not found: $config${NC}"
        exit 1
    }

    if [ "$mode" == "release" ]; then
        echo -e "${YELLOW}Running in release mode...${NC}"
        sudo target/release/sniproxy-server -c "$config"
    else
        echo -e "${YELLOW}Running in debug mode...${NC}"
        sudo target/debug/sniproxy-server -c "$config"
    fi
}

function docker_build() {
    echo -e "${YELLOW}Building Docker image...${NC}"
    docker-compose build
}

function docker_run() {
    echo -e "${YELLOW}Starting services with Docker Compose...${NC}"
    docker-compose up -d
    echo -e "${GREEN}Services started:${NC}"
    echo -e "  - SNIProxy: http://localhost:80 (HTTP) and :443 (HTTPS)"
    echo -e "  - Metrics: http://localhost:9000/metrics"
    echo -e "  - Prometheus: http://localhost:9090"
}

function docker_stop() {
    echo -e "${YELLOW}Stopping Docker services...${NC}"
    docker-compose down
}

function clean() {
    echo -e "${YELLOW}Cleaning build artifacts...${NC}"
    cargo clean
    echo -e "${YELLOW}Cleaning Docker artifacts...${NC}"
    docker-compose down -v
}

# Parse command line arguments
COMMAND=""
RELEASE=false
while [[ $# -gt 0 ]]; do
    case $1 in
        build|test|run|docker-build|docker-run|docker-stop|clean|check)
            COMMAND="$1"
            shift
            ;;
        -c|--config)
            CONFIG_FILE="$2"
            shift 2
            ;;
        -r|--release)
            RELEASE=true
            shift
            ;;
        -h|--help)
            print_help
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            print_help
            exit 1
            ;;
    esac
done

# Execute command
case $COMMAND in
    "build")
        check_requirements
        if [ "$RELEASE" = true ]; then
            build release
        else
            build debug
        fi
        ;;
    "test")
        check_requirements
        run_tests
        ;;
    "run")
        check_requirements
        if [ "$RELEASE" = true ]; then
            build release
            run_proxy release "$CONFIG_FILE"
        else
            build debug
            run_proxy debug "$CONFIG_FILE"
        fi
        ;;
    "docker-build")
        docker_build
        ;;
    "docker-run")
        docker_run
        ;;
    "docker-stop")
        docker_stop
        ;;
    "clean")
        clean
        ;;
    "check")
        check_requirements
        run_tests
        ;;
    *)
        print_help
        exit 1
        ;;
esac

