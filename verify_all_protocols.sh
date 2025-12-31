#!/bin/bash
# SNIProxy Protocol Verification Script
# Tests all supported protocols end-to-end

set +e

echo "======================================"
echo "SNIProxy Protocol Verification"
echo "======================================"
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m' # No Color

PASSED=0
FAILED=0

run_test() {
    local test_name=$1
    local test_pattern=$2

    echo -e "${BLUE}Testing: ${test_name}${NC}"

    if cargo test --release "$test_pattern" -- --nocapture --test-threads=1 2>&1 | grep -q "ok"; then
        echo -e "${GREEN}✅ PASSED${NC}"
        ((PASSED++))
    else
        echo -e "${RED}❌ FAILED${NC}"
        ((FAILED++))
    fi
    echo ""
}

echo "1️⃣  Protocol Detection Tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run_test "HTTP/1.0 Detection" "test_http10_protocol_detection"
run_test "HTTP/1.1 Detection" "test_http11_protocol_detection"
run_test "HTTP/2 Preface Detection" "test_http2_preface_detection"
run_test "HTTP/2 TLS with ALPN" "test_http2_tls_with_alpn"
run_test "HTTP/3 ALPN Detection" "test_http3_alpn_detection"

echo "2️⃣  Live End-to-End Traffic Tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run_test "HTTP/1.1 Full Traffic" "test_comprehensive_http11_traffic"
run_test "HTTP/2 Traffic" "test_comprehensive_http2_traffic"
run_test "WebSocket Traffic" "test_comprehensive_websocket_traffic"
run_test "gRPC Traffic" "test_comprehensive_grpc_traffic"

echo "3️⃣  Protocol Features Tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run_test "Host Header Extraction (HTTP/1.0)" "test_host_header_extraction_http10"
run_test "Host Header Extraction (HTTP/1.1)" "test_host_header_extraction_http11"
run_test "Case Insensitive Headers" "test_case_insensitive_host_header"
run_test "ALPN Extraction (HTTP/2)" "test_alpn_extraction_various_protocols"
run_test "SNI Extraction (TLS)" "test_sni_extraction_various_domains"
run_test "WebSocket Upgrade" "test_websocket_upgrade_request"
run_test "gRPC Content-Type Detection" "test_grpc_detection_via_content_type"
run_test "gRPC with h2 ALPN" "test_grpc_with_h2_alpn"

echo "4️⃣  Stress & Concurrent Tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run_test "High Volume HTTP/1.1" "test_comprehensive_high_volume_http11"
run_test "Concurrent Mixed Protocols" "test_comprehensive_concurrent_mixed_protocols"
run_test "Multiple Concurrent Connections" "test_multiple_concurrent_connections"

echo "5️⃣  TLS/SNI Tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run_test "TLS SNI Proxy Connection" "test_tls_sni_proxy_accepts_connection"
run_test "TLS Version Compatibility" "test_tls_version_compatibility"

echo "6️⃣  Edge Cases & Error Handling"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run_test "Malformed Requests" "test_malformed_requests"
run_test "Large Headers" "test_large_headers"
run_test "Edge Case Domains" "test_edge_case_domains"
run_test "Mixed Protocol Scenarios" "test_mixed_protocol_scenarios"

echo ""
echo "======================================"
echo "VERIFICATION SUMMARY"
echo "======================================"
echo -e "${GREEN}✅ Passed: $PASSED${NC}"
echo -e "${RED}❌ Failed: $FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}🎉 ALL PROTOCOLS VERIFIED - PRODUCTION READY${NC}"
    exit 0
else
    echo -e "${RED}⚠️  Some tests failed - review above${NC}"
    exit 1
fi
