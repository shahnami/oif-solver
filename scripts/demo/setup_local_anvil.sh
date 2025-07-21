#!/bin/bash
# setup_local_anvil.sh - Deploy local Anvil node with EIP-7683 contracts and tokens

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
CHAIN_ID=31337
PORT=8545
PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
PUBLIC_KEY="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
USER_KEY="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
USER_ADDR="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"

echo -e "${BLUE}🔧 Setting up Local Anvil with EIP-7683 Contracts${NC}"
echo "================================================="

# Function to check if port is in use
check_port() {
    if lsof -Pi :$1 -sTCP:LISTEN -t >/dev/null 2>&1; then
        echo -e "${YELLOW}Port $1 is in use. Killing existing process...${NC}"
        kill -9 $(lsof -t -i:$1) 2>/dev/null || true
        sleep 2
    fi
}

# Function to start anvil
start_anvil() {
    echo -e "${YELLOW}🚀 Starting Anvil on port $PORT...${NC}"
    check_port $PORT
    
    ~/.foundry/bin/anvil \
        --chain-id $CHAIN_ID \
        --port $PORT \
        --host 0.0.0.0 \
        --accounts 10 \
        --balance 10000 \
        --gas-limit 30000000 \
        --code-size-limit 50000 \
        --base-fee 0 \
        --gas-price 1000000000 \
        --auto-impersonate \
        --block-time 2 \
        > anvil.log 2>&1 &
    
    ANVIL_PID=$!
    echo $ANVIL_PID > anvil.pid
    
    # Wait for anvil to start
    echo -e "${YELLOW}⏳ Waiting for Anvil to start...${NC}"
    for i in {1..15}; do
        if curl -s -X POST -H "Content-Type: application/json" \
            --data '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
            http://localhost:$PORT > /dev/null; then
            echo -e "${GREEN}✅ Anvil started successfully${NC}"
            return 0
        fi
        sleep 1
    done
    
    echo -e "${RED}❌ Failed to start Anvil${NC}"
    exit 1
}

# Function to deploy contracts
deploy_contracts() {
    echo -e "${YELLOW}📋 Deploying EIP-7683 contracts...${NC}"
    
    # Clone oif-contracts if it doesn't exist
    if [ ! -d "oif-contracts" ]; then
        echo -e "${YELLOW}📥 Cloning oif-contracts repository...${NC}"
        git clone https://github.com/openintentsframework/oif-contracts
    fi
    
    cd oif-contracts
    
    # Create TestToken contract
    cat > /tmp/TestToken.sol << 'EOF'
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

contract TestToken {
    string public name = "TestToken";
    string public symbol = "TEST";
    uint8 public decimals = 18;
    uint256 public totalSupply;
    
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    
    constructor() {
        totalSupply = 0;
    }
    
    function transfer(address to, uint256 value) public returns (bool) {
        require(balanceOf[msg.sender] >= value, "Insufficient balance");
        balanceOf[msg.sender] -= value;
        balanceOf[to] += value;
        emit Transfer(msg.sender, to, value);
        return true;
    }
    
    function approve(address spender, uint256 value) public returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        return true;
    }
    
    function transferFrom(address from, address to, uint256 value) public returns (bool) {
        require(balanceOf[from] >= value, "Insufficient balance");
        require(allowance[from][msg.sender] >= value, "Insufficient allowance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        allowance[from][msg.sender] -= value;
        emit Transfer(from, to, value);
        return true;
    }
    
    function mint(address to, uint256 value) public {
        balanceOf[to] += value;
        totalSupply += value;
        emit Transfer(address(0), to, value);
    }
}
EOF

    echo -e "${BLUE}🪙 Deploying TestToken...${NC}"
    TOKEN_OUTPUT=$(~/.foundry/bin/forge create /tmp/TestToken.sol:TestToken \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        --broadcast 2>&1)
    
    TOKEN_ADDRESS=$(echo "$TOKEN_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
    if [ -z "$TOKEN_ADDRESS" ]; then
        echo -e "${RED}❌ Failed to deploy TestToken${NC}"
        echo "$TOKEN_OUTPUT"
        exit 1
    fi
    echo -e "${GREEN}✅ TestToken deployed: $TOKEN_ADDRESS${NC}"

    echo -e "${BLUE}🏦 Deploying The Compact...${NC}"
    COMPACT_OUTPUT=$(~/.foundry/bin/forge create lib/the-compact/src/TheCompact.sol:TheCompact \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        --broadcast 2>&1)
    
    COMPACT_ADDRESS=$(echo "$COMPACT_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
    if [ -z "$COMPACT_ADDRESS" ]; then
        echo -e "${RED}❌ Failed to deploy The Compact${NC}"
        echo "$COMPACT_OUTPUT"
        exit 1
    fi
    echo -e "${GREEN}✅ The Compact deployed: $COMPACT_ADDRESS${NC}"

    echo -e "${BLUE}🔐 Deploying Permit2...${NC}"
    PERMIT2_ADDRESS="0x000000000022D473030F116dDEE9F6B43aC78BA3"
    
    # Check if Permit2 is already deployed
    PERMIT2_CODE=$(~/.foundry/bin/cast code $PERMIT2_ADDRESS --rpc-url http://localhost:$PORT 2>/dev/null)
    
    if [ "$PERMIT2_CODE" == "0x" ] || [ -z "$PERMIT2_CODE" ]; then
        cd lib/permit2
        ~/.foundry/bin/forge build --use 0.8.17
        PERMIT2_BYTECODE=$(cat out/Permit2.sol/Permit2.json | jq -r '.bytecode.object')
        ~/.foundry/bin/cast rpc anvil_setCode $PERMIT2_ADDRESS $PERMIT2_BYTECODE --rpc-url http://localhost:$PORT
        cd ../..
    fi
    echo -e "${GREEN}✅ Permit2 deployed: $PERMIT2_ADDRESS${NC}"

    echo -e "${BLUE}🔮 Deploying AlwaysYesOracle...${NC}"
    ORACLE_OUTPUT=$(~/.foundry/bin/forge create test/mocks/AlwaysYesOracle.sol:AlwaysYesOracle \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        --broadcast 2>&1)
    
    ORACLE_ADDRESS=$(echo "$ORACLE_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
    if [ -z "$ORACLE_ADDRESS" ]; then
        echo -e "${RED}❌ Failed to deploy AlwaysYesOracle${NC}"
        echo "$ORACLE_OUTPUT"
        exit 1
    fi
    echo -e "${GREEN}✅ AlwaysYesOracle deployed: $ORACLE_ADDRESS${NC}"

    echo -e "${BLUE}⚖️ Deploying InputSettler7683...${NC}"
    SETTLER_OUTPUT=$(~/.foundry/bin/forge create src/input/7683/InputSettler7683.sol:InputSettler7683 \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        --broadcast 2>&1)
    
    SETTLER_ADDRESS=$(echo "$SETTLER_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
    if [ -z "$SETTLER_ADDRESS" ]; then
        echo -e "${RED}❌ Failed to deploy InputSettler7683${NC}"
        echo "$SETTLER_OUTPUT"
        exit 1
    fi
    echo -e "${GREEN}✅ InputSettler7683 deployed: $SETTLER_ADDRESS${NC}"

    echo -e "${BLUE}⚖️ Deploying OutputSettler7683...${NC}"
    OUTPUT_SETTLER_OUTPUT=$(~/.foundry/bin/forge create src/output/coin/OutputSettler7683.sol:OutputInputSettler7683 \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        --broadcast 2>&1)
    
    OUTPUT_SETTLER_ADDRESS=$(echo "$OUTPUT_SETTLER_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
    if [ -z "$OUTPUT_SETTLER_ADDRESS" ]; then
        echo -e "${RED}❌ Failed to deploy OutputSettler7683${NC}"
        echo "$OUTPUT_SETTLER_OUTPUT"
        exit 1
    fi
    echo -e "${GREEN}✅ OutputSettler7683 deployed: $OUTPUT_SETTLER_ADDRESS${NC}"

    cd ..
    rm -f /tmp/TestToken.sol
    
    # Store contract addresses
    export TOKEN_ADDRESS
    export COMPACT_ADDRESS  
    export PERMIT2_ADDRESS
    export ORACLE_ADDRESS
    export SETTLER_ADDRESS
    export OUTPUT_SETTLER_ADDRESS
}

# Function to mint tokens and setup
setup_tokens() {
    echo -e "${YELLOW}💰 Minting tokens and setting up accounts...${NC}"
    
    # Mint tokens to user (for deposits)
    ~/.foundry/bin/cast send $TOKEN_ADDRESS \
        "mint(address,uint256)" \
        $USER_ADDR \
        "100000000000000000000" \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        > /dev/null 2>&1
    
    # Mint tokens to solver (for liquidity)
    ~/.foundry/bin/cast send $TOKEN_ADDRESS \
        "mint(address,uint256)" \
        $PUBLIC_KEY \
        "100000000000000000000" \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        > /dev/null 2>&1
    
    echo -e "${GREEN}✅ Tokens minted:${NC}"
    echo -e "   User ($USER_ADDR): 100 TEST"
    echo -e "   Solver ($PUBLIC_KEY): 100 TEST"
    
    # Approve OutputSettler to spend solver's tokens
    echo -e "${YELLOW}💰 Approving OutputSettler to spend solver's tokens...${NC}"
    ~/.foundry/bin/cast send $TOKEN_ADDRESS \
        "approve(address,uint256)" \
        $OUTPUT_SETTLER_ADDRESS \
        "1000000000000000000000000" \
        --rpc-url http://localhost:$PORT \
        --private-key $PRIVATE_KEY \
        > /dev/null 2>&1
    
    echo -e "${GREEN}✅ OutputSettler approved to spend solver's tokens${NC}"
}

# Function to create configuration
create_config() {
    echo -e "${YELLOW}📝 Creating solver configuration...${NC}"
    
    mkdir -p config
    
    cat > config/local.toml << EOF
# Local development configuration for OIF Solver

# Main solver settings
[solver]
name = "oif-solver-local"
log_level = "debug"
http_port = 8080
metrics_port = 9090

# Plugin configuration
[plugins]

# Discovery plugins
[plugins.discovery.local_discovery]
enabled = true
plugin_type = "eip7683_onchain"

[plugins.discovery.local_discovery.config]
chain_id = $CHAIN_ID
rpc_url = "http://localhost:$PORT"
timeout_ms = 30000
poll_interval_ms = 3000
batch_size = 100
# Local test contracts
input_settler_addresses = ["$SETTLER_ADDRESS"]
output_settler_addresses = ["$OUTPUT_SETTLER_ADDRESS"]
# Event monitoring
monitor_open = true
monitor_finalised = true
monitor_order_purchased = true
# No historical sync for local dev
enable_historical_sync = false

# Delivery plugins
[plugins.delivery.local_delivery]
enabled = true
plugin_type = "evm_ethers"

[plugins.delivery.local_delivery.config]
chain_id = $CHAIN_ID
rpc_url = "http://localhost:$PORT"
# Hardhat test account #0
private_key = "$PRIVATE_KEY"
max_retries = 3
timeout_ms = 30000
enable_eip1559 = true
nonce_management = true
max_pending_transactions = 10

# State plugins
[plugins.state.memory_state]
enabled = true
plugin_type = "memory"

[plugins.state.memory_state.config]
max_entries = 1000

# Order plugins
[plugins.order.eip7683_order]
enabled = true
plugin_type = "eip7683_order"

[plugins.order.eip7683_order.config]
solver_address = "$PUBLIC_KEY"
output_settler_address = "$OUTPUT_SETTLER_ADDRESS"
max_order_age_seconds = 86400
min_fill_deadline_seconds = 300
validate_signatures = false  # Disable for local testing

# Settlement plugins (empty for now)
[plugins.settlement]

# Delivery configuration
[delivery]
strategy = "RoundRobin"
fallback_enabled = false
max_parallel_attempts = 1

# Settlement configuration
[settlement]
default_strategy = "direct"
fallback_strategies = []
profit_threshold_wei = "0"  # No profit requirement for testing

# Discovery configuration
[discovery]
historical_sync = false
realtime_monitoring = true
dedupe_events = true
max_event_age_seconds = 300  # 5 minutes
# Additional configurable fields with defaults
max_events_per_second = 1000
event_buffer_size = 10000
deduplication_window_seconds = 300
max_concurrent_sources = 10

# State configuration
[state]
default_backend = "memory_state"
enable_metrics = true
cleanup_interval_seconds = 300  # 5 minutes
max_concurrent_operations = 100

# Contract addresses for testing
[contracts]
token = "$TOKEN_ADDRESS"
input_settler = "$SETTLER_ADDRESS"
output_settler = "$OUTPUT_SETTLER_ADDRESS"
the_compact = "$COMPACT_ADDRESS"
permit2 = "$PERMIT2_ADDRESS"
oracle = "$ORACLE_ADDRESS"

# Test accounts
[accounts]
solver = "$PUBLIC_KEY"
user = "$USER_ADDR"
user_private_key = "$USER_KEY"
EOF

    cat > .env << EOF
# Local development environment
ETH_RPC_URL=http://localhost:$PORT
ETH_PRIVATE_KEY=$PRIVATE_KEY
RUST_LOG=debug
EOF

    echo -e "${GREEN}✅ Configuration created${NC}"
    echo -e "   Config: config/local.toml"
    echo -e "   Environment: .env"
}

# Function to display summary
show_summary() {
    echo -e "\n${GREEN}🎉 Local Anvil Setup Complete${NC}"
    echo "=============================="
    echo -e "${BLUE}Network:${NC} http://localhost:$PORT (Chain ID: $CHAIN_ID)"
    echo ""
    echo -e "${BLUE}📋 Contract Addresses:${NC}"
    echo -e "   TestToken:         $TOKEN_ADDRESS"
    echo -e "   InputSettler7683:  $SETTLER_ADDRESS"
    echo -e "   OutputSettler7683: $OUTPUT_SETTLER_ADDRESS"
    echo -e "   The Compact:       $COMPACT_ADDRESS"
    echo -e "   Permit2:           $PERMIT2_ADDRESS"
    echo -e "   AlwaysYesOracle:   $ORACLE_ADDRESS"
    echo ""
    echo -e "${BLUE}👥 Test Accounts:${NC}"
    echo -e "   Solver:  $PUBLIC_KEY (100 TEST + 10,000 ETH)"
    echo -e "   User:    $USER_ADDR (100 TEST + 10,000 ETH)"
    echo ""
    echo -e "${YELLOW}📚 Next Steps:${NC}"
    echo -e "   1. Start solver: ${BLUE}cargo run --bin solver-service${NC}"
    echo -e "   2. Send intent:  ${BLUE}./send_intent.sh${NC}"
    echo -e "   3. Monitor:      ${BLUE}./monitor_api.sh${NC}"
    echo ""
    echo -e "${YELLOW}🛑 To stop Anvil:${NC} kill \$(cat anvil.pid) or Ctrl+C"
}

# Function to cleanup
cleanup() {
    echo -e "\n${YELLOW}🧹 Cleaning up...${NC}"
    if [ -f anvil.pid ]; then
        ANVIL_PID=$(cat anvil.pid)
        if kill -0 $ANVIL_PID 2>/dev/null; then
            echo -e "${BLUE}🛑 Stopping Anvil (PID: $ANVIL_PID)...${NC}"
            kill $ANVIL_PID
        fi
        rm -f anvil.pid
    fi
    pkill -f anvil || true
    echo -e "${GREEN}✅ Cleanup complete${NC}"
}

# Handle script termination
trap cleanup EXIT INT TERM

# Check if Foundry is installed
if ! command -v anvil &> /dev/null; then
    echo -e "${RED}❌ Anvil (Foundry) is not installed!${NC}"
    echo -e "${YELLOW}💡 Install with: curl -L https://foundry.paradigm.xyz | bash && foundryup${NC}"
    exit 1
fi

# Main execution
case "${1:-setup}" in
    "setup")
        start_anvil
        deploy_contracts
        setup_tokens
        create_config
        show_summary
        
        # Keep running
        echo -e "${BLUE}📡 Anvil is running. Press Ctrl+C to stop...${NC}"
        while true; do
            sleep 10
        done
        ;;
    "stop")
        cleanup
        ;;
    "status")
        if [ -f anvil.pid ] && kill -0 $(cat anvil.pid) 2>/dev/null; then
            echo -e "${GREEN}✅ Anvil is running (PID: $(cat anvil.pid))${NC}"
            if [ -f config/local.toml ]; then
                show_summary
            fi
        else
            echo -e "${RED}❌ Anvil is not running${NC}"
        fi
        ;;
    "contracts")
        if curl -s http://localhost:$PORT > /dev/null; then
            deploy_contracts
            setup_tokens
            create_config
            show_summary
        else
            echo -e "${RED}❌ Anvil is not running. Run './setup_local_anvil.sh' first${NC}"
        fi
        ;;
    *)
        echo "Usage: $0 [setup|stop|status|contracts]"
        echo ""
        echo "Commands:"
        echo "  setup (default) - Start Anvil and deploy all contracts"
        echo "  stop            - Stop Anvil and cleanup"
        echo "  status          - Check if Anvil is running"
        echo "  contracts       - Only deploy contracts (if Anvil is running)"
        exit 1
        ;;
esac