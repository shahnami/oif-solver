#!/bin/bash
# send_intent.sh - Send a test intent transaction that calls InputSettler7683.open()

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}📤 Sending EIP-7683 Intent Transaction${NC}"
echo "====================================="

# Check if config exists
if [ ! -f "config/local.toml" ]; then
    echo -e "${RED}❌ Configuration not found!${NC}"
    echo -e "${YELLOW}💡 Run './setup_local_anvil.sh' first${NC}"
    exit 1
fi

# Extract contract addresses from config
# Using more robust parsing for nested TOML structure (handle indentation)
TOKEN_ADDRESS=$(grep 'token = ' config/local.toml | head -1 | cut -d'"' -f2)
SETTLER_ADDRESS=$(grep 'input_settler = ' config/local.toml | head -1 | cut -d'"' -f2)
ORACLE_ADDRESS=$(grep 'oracle = ' config/local.toml | head -1 | cut -d'"' -f2)
USER_ADDR=$(grep 'user = ' config/local.toml | head -1 | cut -d'"' -f2)
USER_PRIVATE_KEY=$(grep 'user_private_key = ' config/local.toml | head -1 | cut -d'"' -f2)
SOLVER_ADDR=$(grep 'solver = ' config/local.toml | head -1 | cut -d'"' -f2)

# Configuration
RPC_URL="http://localhost:8545"
AMOUNT="1000000000000000000"  # 1 token
RECIPIENT_ADDR="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"  # Account #2

echo -e "${BLUE}📋 Intent Details:${NC}"
echo -e "   User (depositor): $USER_ADDR"
echo -e "   Solver:          $SOLVER_ADDR"
echo -e "   Recipient:       $RECIPIENT_ADDR"
echo -e "   Amount:          1.0 TEST tokens"
echo -e "   Token:           $TOKEN_ADDRESS"
echo -e "   InputSettler:    $SETTLER_ADDRESS"

# Debug mode - uncomment to see what's being extracted
if [ "$DEBUG" = "1" ]; then
    echo -e "${YELLOW}🔍 Debug Info:${NC}"
    echo -e "   RPC_URL: $RPC_URL"
    echo -e "   TOKEN_ADDRESS length: ${#TOKEN_ADDRESS}"
    echo -e "   USER_ADDR length: ${#USER_ADDR}"
    echo -e "   Config file exists: $([ -f "config/local.toml" ] && echo "Yes" || echo "No")"
fi

# Function to check balances
check_balance() {
    local address=$1
    local name=$2
    
    # Debug mode - show the exact command being run
    if [ "$DEBUG" = "1" ]; then
        echo -e "${YELLOW}   Debug: cast call $TOKEN_ADDRESS \"balanceOf(address)\" $address --rpc-url $RPC_URL${NC}"
    fi
    
    # Filter out debug logs and only get the hex value (starts with 0x)
    local balance_hex=$(cast call $TOKEN_ADDRESS "balanceOf(address)" $address --rpc-url $RPC_URL 2>&1 | grep -E '^0x[0-9a-fA-F]+$' | tail -1)
    
    # Debug: Check if balance_hex is empty
    if [ -z "$balance_hex" ]; then
        # Try without suppressing errors to see what's wrong
        local error_msg=$(cast call $TOKEN_ADDRESS "balanceOf(address)" $address --rpc-url $RPC_URL 2>&1)
        if [ "$DEBUG" = "1" ]; then
            echo -e "${RED}   Error for $name: $error_msg${NC}"
        fi
        echo -e "   $name: 0 TEST (Error: check RPC connection)"
        return
    fi
    
    if [ "$DEBUG" = "1" ]; then
        echo -e "${YELLOW}   Hex balance for $name: $balance_hex${NC}"
    fi
    
    local balance_dec=$(cast to-dec $balance_hex 2>/dev/null || echo "0")
    # Use explicit decimal division instead of exponentiation
    local balance_formatted=$(echo "scale=2; $balance_dec / 1000000000000000000" | bc -l 2>/dev/null || echo "0")
    echo -e "   $name: ${balance_formatted} TEST"
}

# Function to show current balances
show_balances() {
    echo -e "${BLUE}💰 Current Balances:${NC}"
    check_balance $USER_ADDR "User"
    check_balance $SOLVER_ADDR "Solver"
    check_balance $RECIPIENT_ADDR "Recipient"
    check_balance $SETTLER_ADDRESS "InputSettler"
}

# Function to build EIP-7683 intent data
build_intent_data() {
    echo -e "${YELLOW}🔧 Building EIP-7683 intent data...${NC}"
    
    # Calculate expiry (1 hour from now)
    EXPIRY=$(( $(date +%s) + 3600 ))
    
    # Convert values to hex format
    AMOUNT_HEX=$(printf "%064x" $AMOUNT)
    EXPIRY_HEX=$(printf "%064x" $EXPIRY)
    
    # Remove 0x prefix and pad addresses to 32 bytes
    TOKEN_BYTES32="000000000000000000000000${TOKEN_ADDRESS:2}"
    RECIPIENT_BYTES32="000000000000000000000000${RECIPIENT_ADDR:2}"
    ORACLE_BYTES32="000000000000000000000000${ORACLE_ADDRESS:2}"
    
    # Build MandateERC7683 struct
    ORDER_DATA="0x"
    
    # Offset to struct (32 bytes)
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000020"
    
    # expiry (uint32 padded to 32 bytes)
    ORDER_DATA="${ORDER_DATA}${EXPIRY_HEX}"
    
    # localOracle (use AlwaysYesOracle address)
    ORDER_DATA="${ORDER_DATA}${ORACLE_BYTES32}"
    
    # offset to inputs array (0x80 = 128 bytes from struct start)
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000080"
    
    # offset to outputs array (0xe0 = 224 bytes from struct start)
    ORDER_DATA="${ORDER_DATA}00000000000000000000000000000000000000000000000000000000000000e0"
    
    # inputs array: 1 input
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000001"
    
    # Input struct: (token, amount)
    ORDER_DATA="${ORDER_DATA}${TOKEN_BYTES32}"
    ORDER_DATA="${ORDER_DATA}${AMOUNT_HEX}"
    
    # outputs array: 1 output  
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000001"
    
    # offset to first output (0x20 = 32 bytes from outputs array start)
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000020"
    
    # MandateOutput struct:
    # oracle (bytes32) - zero for same-chain
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000000000"
    
    # settler (bytes32) - use same settler for same-chain
    SETTLER_BYTES32="000000000000000000000000${SETTLER_ADDRESS:2}"
    ORDER_DATA="${ORDER_DATA}${SETTLER_BYTES32}"
    
    # chainId - same chain (31337)
    ORDER_DATA="${ORDER_DATA}0000000000000000000000000000000000000000000000000000000000007a69"
    
    # token (bytes32) - same token
    ORDER_DATA="${ORDER_DATA}${TOKEN_BYTES32}"
    
    # amount - same amount
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
    
    # EIP-712 typehash for MandateERC7683
    ORDER_DATA_TYPE="0x532668680e4ed97945ec5ed6aee3633e99abe764fd2d2861903dc7c109b00e82"
    
    echo -e "${GREEN}✅ Intent data built successfully${NC}"
    echo -e "   Data length: ${#ORDER_DATA} characters"
    # Use platform-agnostic date formatting
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        echo -e "   Expiry: $(date -r $EXPIRY)"
    else
        # Linux
        echo -e "   Expiry: $(date -d @$EXPIRY)"
    fi
}

# Function to approve tokens
approve_tokens() {
    echo -e "${YELLOW}🔓 Approving InputSettler to spend tokens...${NC}"
    
    # Check current allowance
    # Filter out debug logs and only get the hex value
    CURRENT_ALLOWANCE=$(cast call $TOKEN_ADDRESS \
        "allowance(address,address)" \
        $USER_ADDR \
        $SETTLER_ADDRESS \
        --rpc-url $RPC_URL 2>&1 | grep -E '^0x[0-9a-fA-F]+$' | tail -1)
    
    # Convert to decimal for comparison
    ALLOWANCE_DEC=$(cast to-dec $CURRENT_ALLOWANCE 2>/dev/null || echo "0")
    REQUIRED_ALLOWANCE=$(cast to-dec $AMOUNT 2>/dev/null || echo "0")
    
    if [ $ALLOWANCE_DEC -lt $REQUIRED_ALLOWANCE ]; then
        echo -e "${BLUE}   Insufficient allowance, approving...${NC}"
        
        APPROVE_TX=$(cast send $TOKEN_ADDRESS \
            "approve(address,uint256)" \
            $SETTLER_ADDRESS \
            "1000000000000000000000000" \
            --rpc-url $RPC_URL \
            --private-key $USER_PRIVATE_KEY 2>&1)
        
        if [ $? -eq 0 ]; then
            echo -e "${GREEN}✅ Approval successful${NC}"
        else
            echo -e "${RED}❌ Approval failed:${NC}"
            echo "$APPROVE_TX"
            exit 1
        fi
    else
        echo -e "${GREEN}✅ Sufficient allowance already exists${NC}"
    fi
}

# Function to send intent transaction
send_intent() {
    echo -e "${YELLOW}🚀 Sending intent transaction...${NC}"
    
    # Call InputSettler7683.open()
    echo -e "${BLUE}   Calling InputSettler7683.open()...${NC}"
    
    INTENT_TX=$(cast send $SETTLER_ADDRESS \
        "open((uint32,bytes32,bytes))" \
        "($EXPIRY,$ORDER_DATA_TYPE,$ORDER_DATA)" \
        --rpc-url $RPC_URL \
        --private-key $USER_PRIVATE_KEY 2>&1)
    
    if [ $? -eq 0 ]; then
        # Extract transaction hash from the output
        # First try to find it in the standard output format
        TX_HASH=$(echo "$INTENT_TX" | grep -o '"transactionHash":"0x[^"]*"' | head -1 | cut -d'"' -f4)
        
        # If that doesn't work, try alternative format
        if [ -z "$TX_HASH" ]; then
            TX_HASH=$(echo "$INTENT_TX" | grep -o '0x[a-fA-F0-9]\{64\}' | head -1)
        fi
        
        echo -e "${GREEN}✅ Intent transaction sent successfully!${NC}"
        
        if [ -n "$TX_HASH" ]; then
            echo -e "${BLUE}   Transaction Hash: $TX_HASH${NC}"
            
            # Extract block number if available
            BLOCK_NUM=$(echo "$INTENT_TX" | grep -o '"blockNumber":"0x[^"]*"' | head -1 | cut -d'"' -f4)
            if [ -n "$BLOCK_NUM" ]; then
                BLOCK_DEC=$(printf "%d" "$BLOCK_NUM" 2>/dev/null || echo "unknown")
                echo -e "${BLUE}   Block: $BLOCK_DEC${NC}"
            fi
        fi
        
        # Wait for transaction to be mined
        echo -e "${YELLOW}⏳ Waiting for transaction to be processed...${NC}"
        sleep 3
        
        return 0
    else
        echo -e "${RED}❌ Intent transaction failed:${NC}"
        echo "$INTENT_TX"
        return 1
    fi
}

# Function to verify transaction
verify_transaction() {
    echo -e "${YELLOW}🔍 Verifying transaction results...${NC}"
    
    # Check if tokens were transferred to InputSettler (escrowed)
    # Filter out debug logs and only get the hex value
    SETTLER_BALANCE_HEX=$(cast call $TOKEN_ADDRESS "balanceOf(address)" $SETTLER_ADDRESS --rpc-url $RPC_URL 2>&1 | grep -E '^0x[0-9a-fA-F]+$' | tail -1)
    SETTLER_BALANCE_DEC=$(cast to-dec $SETTLER_BALANCE_HEX 2>/dev/null || echo "0")
    
    if [ $SETTLER_BALANCE_DEC -ge $AMOUNT ]; then
        echo -e "${GREEN}✅ Tokens successfully escrowed in InputSettler${NC}"
        echo -e "   InputSettler balance: $(echo "scale=2; $SETTLER_BALANCE_DEC / 1000000000000000000" | bc -l 2>/dev/null || echo "unknown") TEST"
    else
        echo -e "${RED}❌ Tokens were not properly escrowed${NC}"
        return 1
    fi
    
    # Check event logs (simplified)
    echo -e "${BLUE}📋 Intent created and ready for solver discovery${NC}"
    echo -e "${YELLOW}💡 The solver should now discover this intent and process it${NC}"
}

# Main execution
main() {
    # Check if Anvil is running
    if ! curl -s $RPC_URL > /dev/null; then
        echo -e "${RED}❌ Anvil is not running on $RPC_URL${NC}"
        echo -e "${YELLOW}💡 Run './setup_local_anvil.sh' first${NC}"
        exit 1
    fi
    
    echo -e "${BLUE}🔍 Checking prerequisites...${NC}"
    
    # Verify contracts are deployed
    if ! cast code $TOKEN_ADDRESS --rpc-url $RPC_URL | grep -q "0x"; then
        echo -e "${RED}❌ TestToken not deployed at $TOKEN_ADDRESS${NC}"
        exit 1
    fi
    
    if ! cast code $SETTLER_ADDRESS --rpc-url $RPC_URL | grep -q "0x"; then
        echo -e "${RED}❌ InputSettler7683 not deployed at $SETTLER_ADDRESS${NC}"
        exit 1
    fi
    
    echo -e "${GREEN}✅ All contracts verified${NC}"
    
    # Show initial balances
    echo ""
    echo -e "${BLUE}📊 BEFORE Intent Creation:${NC}"
    show_balances
    
    # Build intent data
    echo ""
    build_intent_data
    
    # Approve tokens
    echo ""
    approve_tokens
    
    # Send intent
    echo ""
    send_intent
    
    # Verify results
    echo ""
    verify_transaction
    
    # Show final balances
    echo ""
    echo -e "${BLUE}📊 AFTER Intent Creation:${NC}"
    show_balances
    
    echo ""
    echo -e "${GREEN}🎉 Intent Transaction Complete!${NC}"
    echo -e "${YELLOW}📡 Monitor the solver with: ./monitor_api.sh${NC}"
    echo -e "${YELLOW}🔍 Check discovery events at: http://localhost:8080/api/v1/discovery/stats${NC}"
}

# Handle different commands
case "${1:-send}" in
    "send")
        main
        ;;
    "balances")
        if [ -f "config/local.toml" ]; then
            TOKEN_ADDRESS=$(grep 'token = ' config/local.toml | head -1 | cut -d'"' -f2)
            SETTLER_ADDRESS=$(grep 'input_settler = ' config/local.toml | head -1 | cut -d'"' -f2)
            USER_ADDR=$(grep 'user = ' config/local.toml | head -1 | cut -d'"' -f2)
            SOLVER_ADDR=$(grep 'solver = ' config/local.toml | head -1 | cut -d'"' -f2)
            RECIPIENT_ADDR="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"  # Account #2
            show_balances
        else
            echo -e "${RED}❌ Configuration not found!${NC}"
        fi
        ;;
    "approve")
        if [ -f "config/local.toml" ]; then
            TOKEN_ADDRESS=$(grep 'token = ' config/local.toml | head -1 | cut -d'"' -f2)
            SETTLER_ADDRESS=$(grep 'input_settler = ' config/local.toml | head -1 | cut -d'"' -f2)
            USER_ADDR=$(grep 'user = ' config/local.toml | head -1 | cut -d'"' -f2)
            USER_PRIVATE_KEY=$(grep 'user_private_key = ' config/local.toml | head -1 | cut -d'"' -f2)
            AMOUNT="1000000000000000000"  # 1 token
            approve_tokens
        else
            echo -e "${RED}❌ Configuration not found!${NC}"
        fi
        ;;
    *)
        echo "Usage: $0 [send|balances|approve]"
        echo ""
        echo "Commands:"
        echo "  send (default) - Send a complete intent transaction"
        echo "  balances       - Check current token balances"
        echo "  approve        - Just approve tokens (no intent)"
        exit 1
        ;;
esac