#!/bin/bash
# test_api.sh - Fixed API testing script for OIF Solver Service

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

API_BASE="http://localhost:8080"
CHAIN_ID=1

# Test accounts from Anvil
SENDER_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
RECEIVER_ADDRESS="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"

echo -e "${BLUE}🧪 OIF Solver Service API Test Suite${NC}"
echo "===================================="

# Function to check if API is running
check_api() {
    echo -e "${YELLOW}🔍 Checking if API is running...${NC}"
    if curl -s "$API_BASE/health" > /dev/null; then
        echo -e "${GREEN}✅ API is running at $API_BASE${NC}"
        return 0
    else
        echo -e "${RED}❌ API is not responding at $API_BASE${NC}"
        echo -e "${YELLOW}💡 Make sure to run: ./setup_local_test.sh${NC}"
        exit 1
    fi
}

# Function to test health endpoint
test_health() {
    echo -e "\n${BLUE}🏥 Testing Health Endpoint${NC}"
    echo "------------------------"
    
    status_code=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE/health")
    
    if [ "$status_code" = "200" ]; then
        echo -e "${GREEN}✅ Health check passed (HTTP $status_code)${NC}"
    else
        echo -e "${RED}❌ Health check failed (HTTP $status_code)${NC}"
    fi
}

# Function to test plugin health
test_plugin_health() {
    echo -e "\n${BLUE}🔌 Testing Plugin Health Endpoint${NC}"
    echo "--------------------------------"
    
    response=$(curl -s "$API_BASE/api/v1/plugins/health")
    status_code=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE/api/v1/plugins/health")
    
    if [ "$status_code" = "200" ]; then
        echo -e "${GREEN}✅ Plugin health check passed (HTTP $status_code)${NC}"
        echo -e "${BLUE}Plugin status:${NC}"
        echo "$response" | jq '.' 2>/dev/null || echo "$response"
    else
        echo -e "${RED}❌ Plugin health check failed (HTTP $status_code)${NC}"
        echo "Response: $response"
    fi
}

# Function to send a test transaction
send_test_transaction() {
    local test_name="$1"
    local to_address="$2"
    local value="$3"
    local priority="${4:-normal}"
    local data="${5:-}"
    
    echo -e "\n${BLUE}🚀 Testing Transaction Delivery: $test_name${NC}"
    echo "--------------------------------------"
    
    # Build JSON payload
    local json_payload
    if [ -n "$data" ]; then
        json_payload="{\"to\":\"$to_address\",\"value\":$value,\"gas_limit\":21000,\"chain_id\":$CHAIN_ID,\"priority\":\"$priority\",\"order_id\":\"test_$(date +%s)\",\"user\":\"$SENDER_ADDRESS\",\"data\":\"$data\"}"
    else
        json_payload="{\"to\":\"$to_address\",\"value\":$value,\"gas_limit\":21000,\"chain_id\":$CHAIN_ID,\"priority\":\"$priority\",\"order_id\":\"test_$(date +%s)\",\"user\":\"$SENDER_ADDRESS\"}"
    fi
    
    echo -e "${YELLOW}📤 Sending transaction...${NC}"
    echo "To: $to_address"
    echo "Value: $value wei"
    echo "Priority: $priority"
    
    response=$(curl -s -X POST "$API_BASE/api/v1/deliver" \
        -H "Content-Type: application/json" \
        -d "$json_payload")
    
    status_code=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$API_BASE/api/v1/deliver" \
        -H "Content-Type: application/json" \
        -d "$json_payload")
    
    if [ "$status_code" = "200" ]; then
        echo -e "${GREEN}✅ Transaction submitted successfully (HTTP $status_code)${NC}"
        
        # Extract transaction hash
        tx_hash=$(echo "$response" | jq -r '.tx_hash' 2>/dev/null)
        if [ "$tx_hash" != "null" ] && [ -n "$tx_hash" ]; then
            echo -e "${BLUE}📋 Transaction Hash: $tx_hash${NC}"
            
            # Test transaction status endpoint
            test_transaction_status "$tx_hash"
            return 0
        else
            echo -e "${YELLOW}⚠️ Could not extract transaction hash from response${NC}"
            echo "Response: $response"
        fi
    else
        echo -e "${RED}❌ Transaction failed (HTTP $status_code)${NC}"
        echo "Response: $response"
        return 1
    fi
}

# Function to test transaction status
test_transaction_status() {
    local tx_hash="$1"
    
    echo -e "\n${BLUE}📊 Testing Transaction Status${NC}"
    echo "----------------------------"
    
    echo -e "${YELLOW}🔍 Checking status for: $tx_hash${NC}"
    
    # Wait a moment for the transaction to be processed
    sleep 2
    
    response=$(curl -s "$API_BASE/api/v1/delivery/$tx_hash/status?chain_id=$CHAIN_ID")
    status_code=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE/api/v1/delivery/$tx_hash/status?chain_id=$CHAIN_ID")
    
    if [ "$status_code" = "200" ]; then
        echo -e "${GREEN}✅ Transaction status retrieved (HTTP $status_code)${NC}"
        echo -e "${BLUE}Status details:${NC}"
        echo "$response" | jq '.' 2>/dev/null || echo "$response"
    elif [ "$status_code" = "404" ]; then
        echo -e "${YELLOW}⚠️ Transaction not found (HTTP $status_code)${NC}"
        echo "This might be normal if the transaction is very new"
    else
        echo -e "${RED}❌ Failed to get transaction status (HTTP $status_code)${NC}"
        echo "Response: $response"
    fi
}

# Function to run all tests
run_all_tests() {
    check_api
    test_health
    test_plugin_health
    
    # Test 1: Zero value transaction
    send_test_transaction "Zero ETH Transfer" "$RECEIVER_ADDRESS" "0" "normal"
    
    # Test 2: Small ETH transfer
    send_test_transaction "0.01 ETH Transfer" "$RECEIVER_ADDRESS" "10000000000000000" "normal"
    
    # Test 3: Higher priority transaction
    send_test_transaction "High Priority Transfer" "$RECEIVER_ADDRESS" "1000000000000000" "high"
    
    # Test 4: Transaction with data
    send_test_transaction "Transaction with Data" "$RECEIVER_ADDRESS" "0" "normal" "0x48656c6c6f20576f726c64"
    
    echo -e "\n${GREEN}🎉 Test suite completed!${NC}"
}

# Function to test specific transaction hash
test_specific_tx() {
    local tx_hash="$1"
    if [ -z "$tx_hash" ]; then
        echo -e "${RED}❌ Please provide a transaction hash${NC}"
        echo "Usage: $0 status <tx_hash>"
        exit 1
    fi
    
    test_transaction_status "$tx_hash"
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [command] [args...]"
    echo ""
    echo "Commands:"
    echo "  run (default)     - Run all tests"
    echo "  health            - Test health endpoint only"
    echo "  plugins           - Test plugin health only"
    echo "  send [to] [value] [priority] [data] - Send custom transaction"
    echo "  status <tx_hash>  - Check specific transaction status"
    echo "  -h, --help        - Show this help"
    echo ""
    echo "Examples:"
    echo "  $0                                    # Run all tests"
    echo "  $0 send 0x742d35... 1000000000000000000 high  # Send 1 ETH"
    echo "  $0 status 0x1234...                  # Check tx status"
    exit 0
}

# Handle help first
if [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
    show_usage
fi

# Main execution
COMMAND="${1:-run}"

if [ "$COMMAND" = "health" ]; then
    check_api
    test_health

elif [ "$COMMAND" = "plugins" ]; then
    check_api
    test_plugin_health

elif [ "$COMMAND" = "send" ]; then
    check_api
    send_test_transaction "Manual Test" "${2:-$RECEIVER_ADDRESS}" "${3:-0}" "${4:-normal}" "${5:-}"

elif [ "$COMMAND" = "status" ]; then
    check_api
    test_specific_tx "$2"

elif [ "$COMMAND" = "run" ] || [ -z "$COMMAND" ]; then
    run_all_tests

else
    echo -e "${RED}❌ Unknown command: $COMMAND${NC}"
    show_usage
fi