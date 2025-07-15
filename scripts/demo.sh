#!/bin/bash

# Test script to verify cross-chain intent balances
# This script checks balances on both origin and destination chains

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Chain configuration
ORIGIN_CHAIN_ID=31337
DESTINATION_CHAIN_ID=31338
ORIGIN_PORT=8545
DESTINATION_PORT=8546

# Load addresses from config
ORIGIN_TOKEN=$(grep -A 10 "\[chains.31337.contracts\]" config/local.toml | grep 'test_token' | cut -d'"' -f2)
ORIGIN_SETTLER=$(grep -A 10 "\[chains.31337.contracts\]" config/local.toml | grep '^settler' | cut -d'"' -f2)
ORIGIN_COMPACT=$(grep -A 10 "\[chains.31337.contracts\]" config/local.toml | grep 'the_compact' | cut -d'"' -f2)
DEST_TOKEN=$(grep -A 10 "\[chains.31338.contracts\]" config/local.toml | grep 'test_token' | cut -d'"' -f2)
DEST_SETTLER=$(grep -A 10 "\[chains.31338.contracts\]" config/local.toml | grep 'output_settler' | cut -d'"' -f2)

# Key addresses
SOLVER_ADDR="f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"      # Account #0 (acts as solver)
USER_ADDR="70997970C51812dc3A010C7d01b50e0d17dc79C8"        # Account #1 (deposits tokens)
RECIPIENT_ADDR="3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"  # Account #2 (receives tokens)

echo -e "${BLUE}=== Cross-Chain Intent Balance Test ===${NC}"
echo -e "${BLUE}Origin Chain ($ORIGIN_CHAIN_ID):${NC}"
echo "  Token: $ORIGIN_TOKEN"
echo "  Settler: $ORIGIN_SETTLER"
echo -e "${BLUE}Destination Chain ($DESTINATION_CHAIN_ID):${NC}"
echo "  Token: $DEST_TOKEN"
echo "  Settler: $DEST_SETTLER"
echo ""
echo "Solver: 0x$SOLVER_ADDR"
echo "User (depositor): 0x$USER_ADDR"
echo "Recipient: 0x$RECIPIENT_ADDR"
echo ""

# Function to get token balance
get_balance() {
    local token=$1
    local address=$2
    local rpc_url=$3
    local balance_hex=$(~/.foundry/bin/cast call $token "balanceOf(address)" $address --rpc-url $rpc_url 2>/dev/null)
    local balance_dec=$(~/.foundry/bin/cast --to-dec $balance_hex)
    echo $balance_dec
}

# Function to format balance for display
format_balance() {
    local balance=$1
    echo "scale=2; $balance / 10^18" | bc -l | sed 's/\.00$//'
}

# Function to check balances on both chains
check_balances() {
    local title=$1
    
    echo -e "${YELLOW}--- $title ---${NC}"
    
    # Origin chain balances
    echo -e "${BLUE}Origin Chain ($ORIGIN_CHAIN_ID):${NC}"
    SOLVER_ORIGIN=$(get_balance $ORIGIN_TOKEN "0x$SOLVER_ADDR" "http://localhost:$ORIGIN_PORT")
    USER_ORIGIN=$(get_balance $ORIGIN_TOKEN "0x$USER_ADDR" "http://localhost:$ORIGIN_PORT")
    SETTLER_ORIGIN=$(get_balance $ORIGIN_TOKEN $ORIGIN_SETTLER "http://localhost:$ORIGIN_PORT")
    
    echo "  Solver: $(format_balance $SOLVER_ORIGIN) tokens"
    echo "  User: $(format_balance $USER_ORIGIN) tokens"
    echo "  InputSettler7683: $(format_balance $SETTLER_ORIGIN) tokens"
    
    # Destination chain balances
    echo -e "${BLUE}Destination Chain ($DESTINATION_CHAIN_ID):${NC}"
    SOLVER_DEST=$(get_balance $DEST_TOKEN "0x$SOLVER_ADDR" "http://localhost:$DESTINATION_PORT")
    RECIPIENT_DEST=$(get_balance $DEST_TOKEN "0x$RECIPIENT_ADDR" "http://localhost:$DESTINATION_PORT")
    SETTLER_DEST=$(get_balance $DEST_TOKEN $DEST_SETTLER "http://localhost:$DESTINATION_PORT")
    
    echo "  Solver: $(format_balance $SOLVER_DEST) tokens"
    echo "  Recipient: $(format_balance $RECIPIENT_DEST) tokens"
    echo "  OutputSettler7683: $(format_balance $SETTLER_DEST) tokens"
    echo ""
}

# Check if chains are running
echo "Checking if chains are running..."
if ! curl -s -X POST -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
    http://localhost:$ORIGIN_PORT > /dev/null; then
    echo -e "${RED}❌ Origin chain is not running on port $ORIGIN_PORT${NC}"
    echo "Please run: ./scripts/multi-chain-local-dev-oif.sh"
    exit 1
fi

if ! curl -s -X POST -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' \
    http://localhost:$DESTINATION_PORT > /dev/null; then
    echo -e "${RED}❌ Destination chain is not running on port $DESTINATION_PORT${NC}"
    echo "Please run: ./scripts/multi-chain-local-dev-oif.sh"
    exit 1
fi

echo -e "${GREEN}✓ Both chains are running${NC}"
echo ""

# Store initial balances
check_balances "Initial Balances"

# Store for comparison
SOLVER_ORIGIN_BEFORE=$SOLVER_ORIGIN
SOLVER_DEST_BEFORE=$SOLVER_DEST
USER_ORIGIN_BEFORE=$USER_ORIGIN
SETTLER_ORIGIN_BEFORE=$SETTLER_ORIGIN
RECIPIENT_DEST_BEFORE=$RECIPIENT_DEST

# Create cross-chain order
echo -e "${YELLOW}Creating cross-chain order...${NC}"
./scripts/create-order.sh > /dev/null 2>&1

# Wait for order to be processed (fill + settlement)
echo "Waiting for order to be processed (fill + settlement)..."
sleep 15

# Check final balances
check_balances "Final Balances"

# Calculate changes
SOLVER_ORIGIN_CHANGE=$((SOLVER_ORIGIN - SOLVER_ORIGIN_BEFORE))
SOLVER_DEST_CHANGE=$((SOLVER_DEST - SOLVER_DEST_BEFORE))
USER_ORIGIN_CHANGE=$((USER_ORIGIN - USER_ORIGIN_BEFORE))
SETTLER_ORIGIN_CHANGE=$((SETTLER_ORIGIN - SETTLER_ORIGIN_BEFORE))
RECIPIENT_DEST_CHANGE=$((RECIPIENT_DEST - RECIPIENT_DEST_BEFORE))

echo -e "${YELLOW}--- Balance Changes ---${NC}"
echo -e "${BLUE}Solver:${NC}"
echo "  Origin Chain: $(format_balance $SOLVER_ORIGIN_CHANGE) tokens"
echo "  Destination Chain: $(format_balance $SOLVER_DEST_CHANGE) tokens"
echo -e "${BLUE}User (depositor):${NC}"
echo "  Origin Chain: $(format_balance $USER_ORIGIN_CHANGE) tokens"
echo -e "${BLUE}InputSettler7683:${NC}"
echo "  Origin Chain: $(format_balance $SETTLER_ORIGIN_CHANGE) tokens"
echo -e "${BLUE}Recipient:${NC}"
echo "  Destination Chain: $(format_balance $RECIPIENT_DEST_CHANGE) tokens"
echo ""

# Verify cross-chain intent behavior
echo -e "${YELLOW}--- Cross-Chain Intent Verification ---${NC}"
EXPECTED_AMOUNT=1000000000000000000  # 1 token

SUCCESS=true

# Expected behavior for cross-chain intent:
# 1. User deposits tokens into InputSettler7683 via open() call
# 2. Solver provides liquidity on destination chain (solver loses tokens, recipient gains tokens)
# 3. Solver claims user's deposit from InputSettler7683 via finalise() during settlement
# 4. Net result: Tokens moved from origin to destination via solver liquidity

# Check if recipient received tokens on destination chain
if [ "$RECIPIENT_DEST_CHANGE" -eq "$EXPECTED_AMOUNT" ]; then
    echo -e "${GREEN}✓ Recipient received tokens on destination chain: 1 token${NC}"
else
    echo -e "${RED}✗ Recipient did not receive expected tokens on destination chain${NC}"
    echo "  Expected: 1, Got: $(format_balance $RECIPIENT_DEST_CHANGE)"
    SUCCESS=false
fi

# Check if solver spent tokens on destination chain (providing liquidity)
if [ "$SOLVER_DEST_CHANGE" -eq "-$EXPECTED_AMOUNT" ]; then
    echo -e "${GREEN}✓ Solver provided liquidity on destination chain: -1 token${NC}"
else
    echo -e "${RED}✗ Solver liquidity provision on destination chain incorrect${NC}"
    echo "  Expected: -1, Got: $(format_balance $SOLVER_DEST_CHANGE)"
    SUCCESS=false
fi

# Check if solver claimed tokens on origin chain (settlement)
if [ "$SOLVER_ORIGIN_CHANGE" -eq "$EXPECTED_AMOUNT" ]; then
    echo -e "${GREEN}✓ Solver claimed tokens on origin chain during settlement: +1 token${NC}"
else
    echo -e "${RED}✗ Solver did not claim expected tokens on origin chain${NC}"
    echo "  Expected: +1, Got: $(format_balance $SOLVER_ORIGIN_CHANGE)"
    SUCCESS=false
fi

# Check if InputSettler7683 was drained (settlement completed)
if [ "$SETTLER_ORIGIN_CHANGE" -eq "0" ]; then
    echo -e "${GREEN}✓ InputSettler7683 was properly drained during settlement: 0 token${NC}"
else
    echo -e "${RED}✗ InputSettler7683 balance changed unexpectedly${NC}"
    echo "  Expected: 0, Got: $(format_balance $SETTLER_ORIGIN_CHANGE)"
    SUCCESS=false
fi

echo ""
if [ "$SUCCESS" = true ]; then
    echo -e "${GREEN}🎉 Cross-chain intent execution successful!${NC}"
    echo -e "${GREEN}✓ Recipient received tokens on destination chain${NC}"
    echo -e "${GREEN}✓ Solver provided liquidity on destination chain${NC}"
    echo -e "${GREEN}✓ Solver claimed tokens on origin chain (settlement)${NC}"
    echo -e "${GREEN}✓ InputSettler7683 was properly drained (settlement completed)${NC}"
    exit 0
else
    echo -e "${RED}❌ Cross-chain intent execution failed. Please review above.${NC}"
    exit 1
fi