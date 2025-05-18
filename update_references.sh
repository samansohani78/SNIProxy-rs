#!/bin/bash

# Define replacements
declare -A replacements=(
    ["example.com"]="your-domain.com"
    ["example[[:space:]]proxy"]="SNIProxy-rs"
    ["https://github.com/example/"]="https://github.com/samansohani78/"
    ["git@github.com:example/"]="git@github.com:samansohani78/"
    ["# authors = \\[.*\\]"]="authors = [\"Saman Sohani <samansohani78@gmail.com>\"]"
    ["Author:.*"]="Author: Saman Sohani <samansohani78@gmail.com>"
    ["Copyright.*"]="Copyright (c) 2025 Saman Sohani"
    ["your email"]="samansohani78@gmail.com"
    ["your username"]="samansohani78"
    ["your repository"]="SNIProxy-rs"
)

# Files to process
files=(
    "README.md"
    "Cargo.toml"
    "sniproxy/Cargo.toml"
    "sniproxy-core/Cargo.toml"
    "sniproxy-config/Cargo.toml"
    "sniproxy-bin/Cargo.toml"
    "install.sh"
    "uninstall.sh"
    "manage.sh"
    "test-installation.sh"
    "build.sh"
    "CONTRIBUTING.md"
    "sniproxy.service"
    "docker-compose.yml"
)

# Process each file
for file in "${files[@]}"; do
    if [ -f "$file" ]; then
        echo "Processing $file..."
        for pattern in "${!replacements[@]}"; do
            replacement="${replacements[$pattern]}"
            sed -i "s|${pattern}|${replacement}|g" "$file"
        done
    fi
done

# Special handling for workspace Cargo.toml
if [ -f "Cargo.toml" ]; then
    sed -i '/^authors/c\authors = ["Saman Sohani <samansohani78@gmail.com>"]' Cargo.toml
    sed -i '/^repository/c\repository = "https://github.com/samansohani78/SNIProxy-rs"' Cargo.toml
fi
