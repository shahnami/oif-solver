#!/bin/bash

# Multi-chain OIF development setup
# This script sets up two local chains to test cross-chain intents properly

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
ORIGIN_CHAIN_ID=31337
DESTINATION_CHAIN_ID=31338
ORIGIN_PORT=8545
DESTINATION_PORT=8546
PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
PUBLIC_KEY="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

echo -e "${BLUE}=== Multi-Chain OIF Development Setup ===${NC}"
echo -e "${BLUE}Origin Chain: $ORIGIN_CHAIN_ID (port $ORIGIN_PORT)${NC}"
echo -e "${BLUE}Destination Chain: $DESTINATION_CHAIN_ID (port $DESTINATION_PORT)${NC}"
echo ""

# Function to check if port is in use
check_port() {
    if lsof -Pi :$1 -sTCP:LISTEN -t >/dev/null ; then
        echo -e "${YELLOW}Port $1 is already in use. Killing existing process...${NC}"
        kill -9 $(lsof -t -i:$1) 2>/dev/null || true
        sleep 2
    fi
}

# Function to start anvil in background
start_anvil() {
    local chain_id=$1
    local port=$2
    local name=$3
    
    echo -e "${YELLOW}Starting $name (Chain $chain_id) on port $port...${NC}"
    check_port $port
    
    ~/.foundry/bin/anvil \
        --chain-id $chain_id \
        --port $port \
        --accounts 10 \
        --balance 10000 \
        --gas-limit 30000000 \
        --code-size-limit 50000 \
        --base-fee 0 \
        --gas-price 1000000000 \
        --auto-impersonate \
        --block-time 3 \
        > /tmp/anvil_${chain_id}.log 2>&1 &
    
    local anvil_pid=$!
    echo $anvil_pid > /tmp/anvil_${chain_id}.pid
    
    # Wait for anvil to start
    echo "Waiting for $name to start..."
    sleep 3
    
    # Test connection
    if curl -s -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
        http://localhost:$port > /dev/null; then
        echo -e "${GREEN}✓ $name started successfully${NC}"
    else
        echo -e "${RED}✗ Failed to start $name${NC}"
        exit 1
    fi
}

# Function to deploy contracts on a chain
deploy_contracts() {
    local chain_id=$1
    local port=$2
    local name=$3
    local settler_type=$4  # "input" or "output"
    
    echo -e "${YELLOW}Deploying contracts on $name (Chain $chain_id)...${NC}"
    
    # Clone oif-contracts if it doesn't exist
    if [ ! -d "oif-contracts" ]; then
        echo -e "${YELLOW}Cloning oif-contracts repository...${NC}"
        git clone https://github.com/openintentsframework/oif-contracts
    fi
    
    cd oif-contracts
    
    # Create a simple TestToken contract
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
        // Start with 0 total supply, mint as needed
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

    # Deploy TestToken
    echo "Deploying TestToken..."
    TOKEN_OUTPUT=$(~/.foundry/bin/forge create /tmp/TestToken.sol:TestToken \
        --rpc-url http://localhost:$port \
        --private-key $PRIVATE_KEY \
        --broadcast \
        2>&1)
    
    TOKEN_ADDRESS=$(echo "$TOKEN_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
    if [ -z "$TOKEN_ADDRESS" ]; then
        echo -e "${RED}Failed to deploy TestToken on $name${NC}"
        echo "$TOKEN_OUTPUT"
        exit 1
    fi
    
    # Deploy The Compact and Permit2 only on origin chain for escrow
    if [ "$settler_type" = "input" ]; then
        # Deploy The Compact contract
        echo "Deploying The Compact..."
        COMPACT_OUTPUT=$(~/.foundry/bin/forge create lib/the-compact/src/TheCompact.sol:TheCompact \
            --rpc-url http://localhost:$port \
            --private-key $PRIVATE_KEY \
            --broadcast \
            2>&1)
        
        COMPACT_ADDRESS=$(echo "$COMPACT_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
        if [ -z "$COMPACT_ADDRESS" ]; then
            echo -e "${RED}Failed to deploy The Compact on $name${NC}"
            echo "$COMPACT_OUTPUT"
            exit 1
        fi
        
        # Deploy Permit2 to canonical address
        echo "Deploying Permit2 to canonical address..."
        PERMIT2_ADDRESS="0x000000000022D473030F116dDEE9F6B43aC78BA3"
        
        # Check if Permit2 is already deployed
        PERMIT2_CODE=$(~/.foundry/bin/cast code $PERMIT2_ADDRESS --rpc-url http://localhost:$port 2>/dev/null)
        
        if [ "$PERMIT2_CODE" == "0x" ] || [ -z "$PERMIT2_CODE" ]; then
            echo "Permit2 not found at canonical address, deploying..."
            
            # Build Permit2 with the specific version
            cd lib/permit2
            ~/.foundry/bin/forge build --use 0.8.17
            
            # Get the bytecode
            PERMIT2_BYTECODE=$(cat out/Permit2.sol/Permit2.json | jq -r '.bytecode.object')
            
            # Deploy using anvil's setCode to put it at the canonical address
            ~/.foundry/bin/cast rpc anvil_setCode $PERMIT2_ADDRESS $PERMIT2_BYTECODE --rpc-url http://localhost:$port
            
            cd ../..
            echo "✓ Permit2 deployed at canonical address: $PERMIT2_ADDRESS"
        else
            echo "✓ Permit2 already deployed at: $PERMIT2_ADDRESS"
        fi
    else
        # Destination chain - no escrow contracts needed
        echo "Skipping The Compact and Permit2 deployment on destination chain..."
        COMPACT_ADDRESS=""
        PERMIT2_ADDRESS=""
    fi
    
    # Mint tokens to solver and user accounts
    echo "Minting tokens to accounts..."
    # Mint tokens based on chain type
    if [ "$settler_type" = "input" ]; then
        # Origin chain: Mint to user (account 1) for deposits
        ~/.foundry/bin/cast send $TOKEN_ADDRESS \
            "mint(address,uint256)" \
            "0x70997970C51812dc3A010C7d01b50e0d17dc79C8" \
            "100000000000000000000" \
            --rpc-url http://localhost:$port \
            --private-key $PRIVATE_KEY \
            > /dev/null 2>&1
    else
        # Destination chain: Mint to solver (account 0) for liquidity
        ~/.foundry/bin/cast send $TOKEN_ADDRESS \
            "mint(address,uint256)" \
            $PUBLIC_KEY \
            "100000000000000000000" \
            --rpc-url http://localhost:$port \
            --private-key $PRIVATE_KEY \
            > /dev/null 2>&1
    fi
    
    # Deploy AlwaysYesOracle on origin chain only
    if [ "$settler_type" = "input" ]; then
        echo "Deploying AlwaysYesOracle..."
        ORACLE_OUTPUT=$(~/.foundry/bin/forge create test/mocks/AlwaysYesOracle.sol:AlwaysYesOracle \
            --rpc-url http://localhost:$port \
            --private-key $PRIVATE_KEY \
            --broadcast \
            2>&1)
        
        ORACLE_ADDRESS=$(echo "$ORACLE_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
        if [ -z "$ORACLE_ADDRESS" ]; then
            echo -e "${RED}Failed to deploy AlwaysYesOracle on $name${NC}"
            echo "$ORACLE_OUTPUT"
            exit 1
        fi
        echo "✓ AlwaysYesOracle: $ORACLE_ADDRESS"
    else
        ORACLE_ADDRESS=""
    fi
    
    # Deploy appropriate settler
    if [ "$settler_type" = "input" ]; then
        echo "Deploying InputSettler7683..."
        SETTLER_OUTPUT=$(~/.foundry/bin/forge create src/input/7683/InputSettler7683.sol:InputSettler7683 \
            --rpc-url http://localhost:$port \
            --private-key $PRIVATE_KEY \
            --broadcast \
            2>&1)
        
        SETTLER_ADDRESS=$(echo "$SETTLER_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
        SETTLER_NAME="InputSettler7683"
    else
        echo "Deploying OutputSettler7683..."
        SETTLER_OUTPUT=$(~/.foundry/bin/forge create src/output/coin/OutputSettler7683.sol:OutputInputSettler7683 \
            --rpc-url http://localhost:$port \
            --private-key $PRIVATE_KEY \
            --broadcast \
            2>&1)
        
        SETTLER_ADDRESS=$(echo "$SETTLER_OUTPUT" | grep "Deployed to:" | awk '{print $3}')
        SETTLER_NAME="OutputSettler7683"
    fi
    
    if [ -z "$SETTLER_ADDRESS" ]; then
        echo -e "${RED}Failed to deploy $SETTLER_NAME on $name${NC}"
        echo "$SETTLER_OUTPUT"
        exit 1
    fi
    
    # Approve settler to spend tokens
    echo "Approving $SETTLER_NAME to spend TestToken..."
    APPROVE_OUTPUT=$(~/.foundry/bin/cast send $TOKEN_ADDRESS \
        "approve(address,uint256)" \
        $SETTLER_ADDRESS \
        "1000000000000000000000000" \
        --rpc-url http://localhost:$port \
        --private-key $PRIVATE_KEY \
        2>&1)
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}Failed to approve $SETTLER_NAME${NC}"
        echo "$APPROVE_OUTPUT"
        exit 1
    fi
    
    # Verify the approval was successful
    ALLOWANCE=$(~/.foundry/bin/cast call $TOKEN_ADDRESS \
        "allowance(address,address)" \
        $PUBLIC_KEY \
        $SETTLER_ADDRESS \
        --rpc-url http://localhost:$port \
        2>/dev/null)
    
    if [ "$ALLOWANCE" != "0x00000000000000000000000000000000000000000000d3c21bcecceda1000000" ]; then
        echo -e "${RED}Approval verification failed for $SETTLER_NAME${NC}"
        echo "Expected: 0x00000000000000000000000000000000000000000000d3c21bcecceda1000000"
        echo "Got: $ALLOWANCE"
        exit 1
    fi
    
    echo -e "${GREEN}✓ $SETTLER_NAME approval verified${NC}"
    
    cd ..
    
    # Clean up temp file
    rm -f /tmp/TestToken.sol
    
    echo -e "${GREEN}✓ $name deployment complete${NC}"
    echo "  TestToken: $TOKEN_ADDRESS"
    echo "  $SETTLER_NAME: $SETTLER_ADDRESS"
    echo "  The Compact: $COMPACT_ADDRESS"
    echo "  Permit2: $PERMIT2_ADDRESS"
    echo "  AlwaysYesOracle: $ORACLE_ADDRESS"
    
    # Export variables for use in config
    if [ "$settler_type" = "input" ]; then
        export ORIGIN_TOKEN_ADDRESS=$TOKEN_ADDRESS
        export ORIGIN_SETTLER_ADDRESS=$SETTLER_ADDRESS
        export ORIGIN_COMPACT_ADDRESS=$COMPACT_ADDRESS
        export ORIGIN_PERMIT2_ADDRESS=$PERMIT2_ADDRESS
        export ORIGIN_ORACLE_ADDRESS=$ORACLE_ADDRESS
    else
        export DEST_TOKEN_ADDRESS=$TOKEN_ADDRESS
        export DEST_SETTLER_ADDRESS=$SETTLER_ADDRESS
        export DEST_COMPACT_ADDRESS=$COMPACT_ADDRESS
        export DEST_PERMIT2_ADDRESS=$PERMIT2_ADDRESS
    fi
}

# Function to create multi-chain config
create_config() {
    echo -e "${YELLOW}Creating multi-chain configuration...${NC}"
    
    cat > config/local.toml << EOF
# THIS IS AUTO-GENERATED BY setup_local_nodes.sh. ANY CHANGES HERE WILL LIKELY BE OVERWRITTEN

[solver]
name = "oif-solver-multi-chain"
private_key = "$PRIVATE_KEY"

[chains]

[chains.$ORIGIN_CHAIN_ID]
name = "Origin Chain"
rpc_url = "http://localhost:$ORIGIN_PORT"
confirmations = 0
block_time = 1

[chains.$ORIGIN_CHAIN_ID.contracts]
settler = "$ORIGIN_SETTLER_ADDRESS"
test_token = "$ORIGIN_TOKEN_ADDRESS"
the_compact = "$ORIGIN_COMPACT_ADDRESS"
permit2 = "$ORIGIN_PERMIT2_ADDRESS"
oracle = "$ORIGIN_ORACLE_ADDRESS"

[chains.$ORIGIN_CHAIN_ID.contracts.custom]

[chains.$DESTINATION_CHAIN_ID]
name = "Destination Chain"
rpc_url = "http://localhost:$DESTINATION_PORT"
confirmations = 0
block_time = 1

[chains.$DESTINATION_CHAIN_ID.contracts]
output_settler = "$DEST_SETTLER_ADDRESS"
test_token = "$DEST_TOKEN_ADDRESS"

[chains.$DESTINATION_CHAIN_ID.contracts.custom]

[discovery]
monitor_chains = [$ORIGIN_CHAIN_ID]
start_blocks = { $ORIGIN_CHAIN_ID = 0 }
poll_interval_secs = 2
enable_offchain = false
offchain_endpoints = []

[settlement]
default_type = "Direct"
poll_interval_secs = 5
max_attempts = 3

[settlement.strategies.Direct]
gas_limit = 300000
gas_multiplier = 1.2
default_expiry_duration = 3600
solver_address = "$PUBLIC_KEY"
oracle_address = "$ORIGIN_ORACLE_ADDRESS"

[settlement.strategies.Direct.settler_addresses]
$ORIGIN_CHAIN_ID = "$ORIGIN_SETTLER_ADDRESS"
$DESTINATION_CHAIN_ID = "$DEST_SETTLER_ADDRESS"

[state]
storage_backend = "memory"
max_queue_size = 1000
recover_on_startup = false

[delivery]
default_service = "rpc"

[delivery.services.rpc]
api_key = "demo"
max_retries = 3

[delivery.services.rpc.gas_strategy]
type = "standard"

[delivery.services.rpc.endpoints]
$ORIGIN_CHAIN_ID = "http://localhost:$ORIGIN_PORT"
$DESTINATION_CHAIN_ID = "http://localhost:$DESTINATION_PORT"

[strategy.profitability]
min_profit_bps = 0
include_gas_costs = false
price_slippage_tolerance = 0.01

[strategy.risk]
blocked_tokens = []

[strategy.fallback]
enabled = false
delay_before_fallback_secs = 300
strategies = []

[monitoring]
enabled = true
metrics_port = 9090
health_port = 8080
log_level = "debug"
EOF
    
    echo -e "${GREEN}✓ Multi-chain configuration created${NC}"
}

# Function to create cross-chain test order script
create_cross_chain_order_script() {
    echo -e "${YELLOW}Creating cross-chain order script...${NC}"
    
    cat > scripts/create-order.sh << 'EOF'
#!/bin/bash

# THIS IS AUTO-GENERATED BY setup_local_nodes.sh. ANY CHANGES HERE WILL LIKELY BE OVERWRITTEN

# Create a cross-chain order from origin to destination chain
set -e

# Load addresses from config
ORIGIN_TOKEN=$(grep -A 15 "\[chains.31337.contracts\]" config/local.toml | grep 'test_token' | cut -d'"' -f2)
ORIGIN_SETTLER=$(grep -A 15 "\[chains.31337.contracts\]" config/local.toml | grep '^settler' | cut -d'"' -f2)
ORIGIN_COMPACT=$(grep -A 15 "\[chains.31337.contracts\]" config/local.toml | grep 'the_compact' | cut -d'"' -f2)
ORIGIN_ORACLE=$(grep -A 15 "\[chains.31337.contracts\]" config/local.toml | grep 'oracle' | cut -d'"' -f2)
DEST_TOKEN=$(grep -A 15 "\[chains.31338.contracts\]" config/local.toml | grep 'test_token' | cut -d'"' -f2)
DEST_SETTLER=$(grep -A 15 "\[chains.31338.contracts\]" config/local.toml | grep 'output_settler' | cut -d'"' -f2)

# User and recipient addresses (user will deposit, solver will provide liquidity)
USER_ADDR="70997970C51812dc3A010C7d01b50e0d17dc79C8"    # Account #1 (user depositing)
SOLVER_ADDR="f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"   # Account #0 (solver)
RECIPIENT_ADDR="3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"       # Account #2 (recipient)
USER_PRIVATE_KEY="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"  # Account #1 private key

echo "Creating cross-chain order with Compact escrow..."
echo "Origin Token: $ORIGIN_TOKEN"
echo "Origin Settler: $ORIGIN_SETTLER"
echo "Origin Compact: $ORIGIN_COMPACT"
echo "Origin Oracle: $ORIGIN_ORACLE"
echo "Destination Token: $DEST_TOKEN"
echo "Destination Settler: $DEST_SETTLER"
echo "User (depositor): 0x$USER_ADDR"
echo "Solver: 0x$SOLVER_ADDR"
echo "Recipient: 0x$RECIPIENT_ADDR"

# Amount to transfer (1 token)
AMOUNT="1000000000000000000"

# Step 1: User approves InputSettler7683 to spend tokens
echo "Step 1: User approves InputSettler7683 to spend tokens..."
~/.foundry/bin/cast send $ORIGIN_TOKEN \
  "approve(address,uint256)" \
  $ORIGIN_SETTLER \
  "1000000000000000000000000" \
  --rpc-url http://localhost:8545 \
  --private-key $USER_PRIVATE_KEY

# Create order data with destination chain information
FILL_DEADLINE=$(( $(date +%s) + 3600 ))  # 1 hour from now

# Build MandateOutput for destination chain using proper ABI encoding
# This should match the format used in the working single-chain order

# Convert values to hex format
AMOUNT_HEX=$(printf "%064x" $AMOUNT)
CHAIN_ID_HEX=$(printf "%08x" 31338)  # uint32 for chain ID

# Remove 0x prefix and pad addresses to 32 bytes
DEST_SETTLER_BYTES32="000000000000000000000000${DEST_SETTLER:2}"
DEST_TOKEN_BYTES32="000000000000000000000000${DEST_TOKEN:2}"
RECIPIENT_BYTES32="000000000000000000000000${RECIPIENT_ADDR}"

# Create complete MandateERC7683 struct like the working single-chain version
ORDER_DATA="0x"

# Offset to struct
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000020"

# expiry (uint32 padded to 32 bytes) - 1 hour from now
EXPIRY=$(( $(date +%s) + 3600 ))
ORDER_DATA="${ORDER_DATA}$(printf '%064x' $EXPIRY)"

# localOracle (use AlwaysYesOracle address)
ORACLE_BYTES32="000000000000000000000000${ORIGIN_ORACLE:2}"
ORDER_DATA="${ORDER_DATA}${ORACLE_BYTES32}"

# offset to inputs array (0x80 = 128 bytes from struct start)
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000080"

# offset to outputs array (0xe0 = 224 bytes from struct start: 128 + 32 + 64)
ORDER_DATA="${ORDER_DATA}00000000000000000000000000000000000000000000000000000000000000e0"

# inputs array: 1 input
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000001"

# Input struct: (token, amount)
# token (uint256) - origin token as uint256
ORIGIN_TOKEN_BYTES32="000000000000000000000000${ORIGIN_TOKEN:2}"
ORDER_DATA="${ORDER_DATA}${ORIGIN_TOKEN_BYTES32}"

# amount - 1 token
ORDER_DATA="${ORDER_DATA}${AMOUNT_HEX}"

# outputs array: 1 output
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000001"

# offset to first output (0x20 = 32 bytes from outputs array start)
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000020"

# MandateOutput struct:
# oracle (bytes32) - zero for no oracle
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000000"

# settler (bytes32) - use the destination settler address for cross-chain
ORDER_DATA="${ORDER_DATA}${DEST_SETTLER_BYTES32}"

# chainId - destination chain (31338)
ORDER_DATA="${ORDER_DATA}00000000000000000000000000000000000000000000000000000000$CHAIN_ID_HEX"

# token (bytes32) - destination token
ORDER_DATA="${ORDER_DATA}${DEST_TOKEN_BYTES32}"

# amount - 1 token
ORDER_DATA="${ORDER_DATA}${AMOUNT_HEX}"

# recipient (bytes32)
ORDER_DATA="${ORDER_DATA}${RECIPIENT_BYTES32}"

# offset to call data (0x100 = 256 bytes from output struct start)
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000100"

# offset to context data (0x120 = 288 bytes from output struct start)
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000120"

# call data - empty (length = 0)
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000000"

# context data - empty (length = 0)
ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000000"

# The EIP-712 typehash for MandateERC7683
ORDER_DATA_TYPE="0x532668680e4ed97945ec5ed6aee3633e99abe764fd2d2861903dc7c109b00e82"

# Step 3: User creates the cross-chain intent order
echo "Step 3: User creates cross-chain intent order..."
echo "Order data length: ${#ORDER_DATA}"
echo "Fill deadline: $FILL_DEADLINE"
echo "Origin settler: $ORIGIN_SETTLER"
echo ""
echo "Note: This order represents a user's intent to move tokens cross-chain."
echo "Tokens are now escrowed in The Compact and the order signals to solvers"
echo "that they can provide liquidity on the destination chain and claim the"
echo "escrowed tokens after fulfillment."
echo ""

# Note: InputSettler7683.open() will pull tokens from user via approval
~/.foundry/bin/cast send $ORIGIN_SETTLER \
  "open((uint32,bytes32,bytes))" \
  "($FILL_DEADLINE,$ORDER_DATA_TYPE,$ORDER_DATA)" \
  --rpc-url http://localhost:8545 \
  --private-key $USER_PRIVATE_KEY

echo "Cross-chain order created successfully!"
EOF
    
    chmod +x scripts/create-order.sh
    echo -e "${GREEN}✓ Cross-chain order script created${NC}"
}

# Function to cleanup
cleanup() {
    echo -e "${YELLOW}Cleaning up...${NC}"
    
    # Kill anvil processes
    if [ -f /tmp/anvil_$ORIGIN_CHAIN_ID.pid ]; then
        kill $(cat /tmp/anvil_$ORIGIN_CHAIN_ID.pid) 2>/dev/null || true
        rm -f /tmp/anvil_$ORIGIN_CHAIN_ID.pid
    fi
    
    if [ -f /tmp/anvil_$DESTINATION_CHAIN_ID.pid ]; then
        kill $(cat /tmp/anvil_$DESTINATION_CHAIN_ID.pid) 2>/dev/null || true
        rm -f /tmp/anvil_$DESTINATION_CHAIN_ID.pid
    fi
    
    # Kill any remaining anvil processes on these ports
    check_port $ORIGIN_PORT
    check_port $DESTINATION_PORT
    
    echo -e "${GREEN}✓ Cleanup complete${NC}"
}

# Handle script termination
trap cleanup EXIT

# Main execution
echo -e "${BLUE}Starting multi-chain setup...${NC}"

# Start both chains
start_anvil $ORIGIN_CHAIN_ID $ORIGIN_PORT "Origin Chain"
start_anvil $DESTINATION_CHAIN_ID $DESTINATION_PORT "Destination Chain"

# Deploy contracts
deploy_contracts $ORIGIN_CHAIN_ID $ORIGIN_PORT "Origin Chain" "input"
deploy_contracts $DESTINATION_CHAIN_ID $DESTINATION_PORT "Destination Chain" "output"

# Create configuration
create_config

# Create cross-chain order script
create_cross_chain_order_script

echo ""
echo -e "${GREEN}=== Multi-Chain Setup Complete ===${NC}"
echo -e "${GREEN}Origin Chain (InputSettler): http://localhost:$ORIGIN_PORT${NC}"
echo -e "${GREEN}Destination Chain (OutputSettler): http://localhost:$DESTINATION_PORT${NC}"
echo ""
echo -e "${YELLOW}To test cross-chain intents:${NC}"
echo -e "${YELLOW}1. Run the solver with: cargo run --bin oif-solver -- --config config/local.toml${NC}"
echo -e "${YELLOW}2. Create a cross-chain order: ./scripts/create-order.sh${NC}"
echo -e "${YELLOW}3. Check balances on both chains${NC}"
echo ""
echo -e "${BLUE}Press Ctrl+C to stop the chains and cleanup${NC}"

# Keep script running
while true; do
    sleep 10
done