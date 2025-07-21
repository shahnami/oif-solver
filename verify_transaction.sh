#\!/bin/bash

# Transaction hash from the logs
TX_HASH="0x96feff6d31decf924ae66ca9813729d406e24b39631626dcd29a3c40612f4ffe"
RPC_URL="http://localhost:8545"

echo "🔍 Verifying transaction: $TX_HASH"
echo "=================================="

# Get transaction details
echo -e "\n📄 Transaction Details:"
cast tx $TX_HASH --rpc-url $RPC_URL

# Get transaction receipt
echo -e "\n📋 Transaction Receipt:"
cast receipt $TX_HASH --rpc-url $RPC_URL

# Check if transaction was successful
STATUS=$(cast receipt $TX_HASH --rpc-url $RPC_URL 2>/dev/null | grep -o '"status":"0x[0-9]*"' | cut -d'"' -f4)
if [ "$STATUS" = "0x1" ]; then
    echo -e "\n✅ Transaction was successful\!"
else
    echo -e "\n❌ Transaction failed or not found"
fi

# Get the recipient's balance to verify they received the funds
RECIPIENT="0x70997970c51812dc3a010c7d01b50e0d17dc79c8"
echo -e "\n💰 Recipient balance:"
cast balance $RECIPIENT --rpc-url $RPC_URL
