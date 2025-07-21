#!/bin/bash
# setup_local_test.sh - Complete local testing environment setup

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 OIF Solver Service - Local Test Environment Setup${NC}"
echo "======================================================="

# Check if Foundry is installed
check_foundry() {
    if ! command -v anvil &> /dev/null; then
        echo -e "${RED}❌ Anvil (Foundry) is not installed!${NC}"
        echo -e "${YELLOW}💡 Install Foundry by running:${NC}"
        echo "   curl -L https://foundry.paradigm.xyz | bash"
        echo "   foundryup"
        exit 1
    fi
    echo -e "${GREEN}✅ Foundry/Anvil is installed${NC}"
}

# Create .env file if it doesn't exist
setup_env() {
    if [ ! -f ".env" ]; then
        echo -e "${YELLOW}📝 Creating .env file with test configuration...${NC}"
        cat > .env << 'EOF'
# Local development configuration for OIF Solver Service
# Using Anvil (local Ethereum node) for testing

# === Ethereum Configuration (Anvil) ===
ETH_PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
ETH_RPC_URL=http://localhost:8545

# === Logging Configuration ===
RUST_LOG=info

# === Test Account Info ===
# Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
# This is the first test account from Anvil with 10,000 ETH
# Private key above corresponds to this address

# === Additional Test Accounts (if needed) ===
# Account 1: 0x70997970C51812dc3A010C7d01b50e0d17dc79C8
# Private:   0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
#
# Account 2: 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC  
# Private:   0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a
EOF
        echo -e "${GREEN}✅ Created .env file with test configuration${NC}"
    else
        echo -e "${GREEN}✅ .env file already exists${NC}"
    fi
}

# Function to start Anvil in background
start_anvil() {
    echo -e "${BLUE}🔧 Starting Anvil (local Ethereum node)...${NC}"
    
    # Kill any existing anvil processes
    pkill -f anvil || true
    sleep 2
    
    # Start Anvil with specific configuration
    anvil \
        --chain-id 1 \
        --port 8545 \
        --host 0.0.0.0 \
        --accounts 10 \
        --balance 10000 \
        --gas-limit 30000000 \
        --gas-price 1000000000 \
        --block-time 1 \
        --silent > anvil.log 2>&1 &
    
    ANVIL_PID=$!
    echo $ANVIL_PID > anvil.pid
    
    # Wait for Anvil to start
    echo -e "${YELLOW}⏳ Waiting for Anvil to start...${NC}"
    for i in {1..10}; do
        if curl -s -X POST http://localhost:8545 -H "Content-Type: application/json" \
           -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' > /dev/null; then
            echo -e "${GREEN}✅ Anvil is running on http://localhost:8545${NC}"
            return 0
        fi
        sleep 1
    done
    
    echo -e "${RED}❌ Failed to start Anvil${NC}"
    exit 1
}

# Function to check Anvil status
check_anvil() {
    if curl -s -X POST http://localhost:8545 -H "Content-Type: application/json" \
       -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' > /dev/null; then
        echo -e "${GREEN}✅ Anvil is running${NC}"
        
        # Get account info
        BALANCE=$(curl -s -X POST http://localhost:8545 \
                  -H "Content-Type: application/json" \
                  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "latest"],"id":1}' \
                  | jq -r '.result')
        
        if [ "$BALANCE" != "null" ]; then
            # Convert from hex to decimal and then to ETH
            BALANCE_WEI=$(printf "%d" "$BALANCE")
            BALANCE_ETH=$(echo "scale=2; $BALANCE_WEI / 1000000000000000000" | bc)
            echo -e "${BLUE}💰 Test account balance: ${BALANCE_ETH} ETH${NC}"
        fi
        
        return 0
    else
        return 1
    fi
}

# Function to build the solver service
build_solver() {
    echo -e "${BLUE}🔨 Building the solver service...${NC}"
    if cargo build --release --bin solver-service; then
        echo -e "${GREEN}✅ Solver service built successfully${NC}"
    else
        echo -e "${RED}❌ Failed to build solver service${NC}"
        exit 1
    fi
}

# Function to run the solver service
run_solver() {
    echo -e "${BLUE}🚀 Starting OIF Solver Service...${NC}"
    echo ""
    echo -e "${YELLOW}📡 API Endpoints:${NC}"
    echo "   - Health check:    http://localhost:8080/health"
    echo "   - Plugin health:   http://localhost:8080/api/v1/plugins/health"
    echo "   - Deliver tx:      POST http://localhost:8080/api/v1/deliver"
    echo "   - Transaction status: GET http://localhost:8080/api/v1/delivery/{tx_hash}/status?chain_id=1"
    echo ""
    echo -e "${YELLOW}💡 Test Account Info:${NC}"
    echo "   - Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    echo "   - Private Key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    echo "   - Balance: 10,000 ETH"
    echo ""
    echo -e "${GREEN}🎯 Ready for testing! Press Ctrl+C to stop${NC}"
    echo ""
    
    # Load environment variables
    export $(grep -v '^#' .env | xargs)
    
    # Run the solver service
    exec cargo run --release --bin solver-service
}

# Function to cleanup processes
cleanup() {
    echo -e "\n${YELLOW}🧹 Cleaning up...${NC}"
    
    # Stop Anvil if it's running
    if [ -f anvil.pid ]; then
        ANVIL_PID=$(cat anvil.pid)
        if kill -0 $ANVIL_PID 2>/dev/null; then
            echo -e "${BLUE}🛑 Stopping Anvil (PID: $ANVIL_PID)...${NC}"
            kill $ANVIL_PID
        fi
        rm -f anvil.pid
    fi
    
    # Clean up any other anvil processes
    pkill -f anvil || true
    
    echo -e "${GREEN}✅ Cleanup complete${NC}"
}

# Function to show test examples
show_test_examples() {
    echo -e "\n${BLUE}📚 Test Examples${NC}"
    echo "================"
    echo ""
    echo -e "${YELLOW}1. Health Check:${NC}"
    echo "curl http://localhost:8080/health"
    echo ""
    echo -e "${YELLOW}2. Plugin Health:${NC}"
    echo "curl http://localhost:8080/api/v1/plugins/health"
    echo ""
    echo -e "${YELLOW}3. Send Test Transaction (0 ETH to self):${NC}"
    cat << 'EOF'
curl -X POST http://localhost:8080/api/v1/deliver \
  -H "Content-Type: application/json" \
  -d '{
    "to": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    "value": 0,
    "gas_limit": 21000,
    "chain_id": 1,
    "priority": "normal",
    "order_id": "test_001",
    "user": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
  }'
EOF
    echo ""
    echo -e "${YELLOW}4. Send 1 ETH Transaction:${NC}"
    cat << 'EOF'
curl -X POST http://localhost:8080/api/v1/deliver \
  -H "Content-Type: application/json" \
  -d '{
    "to": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
    "value": 1000000000000000000,
    "gas_limit": 21000,
    "chain_id": 1,
    "priority": "normal",
    "order_id": "test_002",
    "user": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
  }'
EOF
    echo ""
    echo -e "${YELLOW}5. Check Transaction Status:${NC}"
    echo "curl \"http://localhost:8080/api/v1/delivery/{TX_HASH}/status?chain_id=1\""
    echo ""
}

# Main execution
main() {
    # Set up cleanup trap
    trap cleanup EXIT INT TERM
    
    # Check prerequisites
    check_foundry
    
    # Setup environment
    setup_env
    
    # Handle command line arguments
    case "${1:-run}" in
        "anvil-only")
            start_anvil
            echo -e "${GREEN}✅ Anvil is running. Use 'pkill anvil' to stop it.${NC}"
            echo -e "${BLUE}📋 Anvil logs: tail -f anvil.log${NC}"
            ;;
        "build-only")
            build_solver
            ;;
        "test-examples")
            show_test_examples
            ;;
        "status")
            if check_anvil; then
                show_test_examples
            else
                echo -e "${RED}❌ Anvil is not running${NC}"
                echo "Run './setup_local_test.sh anvil-only' to start it"
            fi
            ;;
        "run"|*)
            # Full setup and run
            start_anvil
            build_solver
            show_test_examples
            run_solver
            ;;
    esac
}

# Show usage if help is requested
if [[ "${1}" == "-h" || "${1}" == "--help" ]]; then
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  run (default)    - Start Anvil, build, and run solver service"
    echo "  anvil-only       - Only start Anvil node"
    echo "  build-only       - Only build the solver service"
    echo "  test-examples    - Show API test examples"
    echo "  status           - Check if Anvil is running and show examples"
    echo "  -h, --help       - Show this help"
    echo ""
    echo "Examples:"
    echo "  $0               # Full setup and run"
    echo "  $0 anvil-only    # Just start Anvil for other tools"
    echo "  $0 status        # Check status and show test commands"
    exit 0
fi

# Run main function
main "$1"