#!/bin/bash

# Test HTTP
echo "Testing HTTP proxy..."
curl -v --proxy http://127.0.0.1:80 http://ip.me/

# Test HTTPS (SNI)
echo -e "\nTesting HTTPS proxy..."
curl -v --proxy http://127.0.0.1:80 https://ip.me/

# Test direct HTTPS
echo -e "\nTesting direct HTTPS connection..."
curl -v https://ip.me/ --resolve ip.me:443:127.0.0.1
