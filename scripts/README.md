# OIF Solver Service - Local Testing Guide

This guide shows you how to run and test the OIF Solver Service with the EVM Ethers plugin using a local Anvil node.

## 🚀 Quick Start

### Prerequisites

1. **Install Foundry** (for Anvil):

   ```bash
   curl -L https://foundry.paradigm.xyz | bash
   foundryup
   ```

2. **Install jq** (for JSON parsing in tests):

   ```bash
   # macOS
   brew install jq

   # Ubuntu/Debian
   sudo apt install jq

   # Or use your package manager
   ```

### Run the Complete Test Environment

1. **Make scripts executable**:

   ```bash
   chmod +x setup_local_test.sh test_api.sh
   ```

2. **Start everything**:
   ```bash
   ./setup_local_test.sh
   ```

This will:

- ✅ Create a `.env` file with test configuration
- ✅ Start Anvil (local Ethereum node) on port 8545
- ✅ Build the solver service
- ✅ Run the solver service on port 8080
- ✅ Show test examples

## 🧪 Testing the API

### Automated Tests

Run the complete test suite:

```bash
./test_api.sh
```

### Interactive Testing

For step-by-step testing:

```bash
./test_api.sh interactive
```

### Individual Tests

```bash
# Health checks
./test_api.sh health
./test_api.sh plugins

# Send transactions
./test_api.sh send 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 1000000000000000000 normal

# Check transaction status
./test_api.sh status 0x1234567890abcdef...

# Monitor continuously
./test_api.sh monitor
```

## 📡 API Endpoints

| Endpoint                         | Method | Description                     |
| -------------------------------- | ------ | ------------------------------- |
| `/health`                        | GET    | Basic health check              |
| `/api/v1/plugins/health`         | GET    | Plugin health and status        |
| `/api/v1/deliver`                | POST   | Submit transaction for delivery |
| `/api/v1/delivery/{hash}/status` | GET    | Get transaction status          |

## 💰 Test Accounts

Anvil provides test accounts with 10,000 ETH each:

```bash
# Primary test account (used by solver)
Address:     0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
Private Key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80

# Secondary test account (for receiving)
Address:     0x70997970C51812dc3A010C7d01b50e0d17dc79C8
Private Key: 0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
```

## 🔧 Manual API Testing

### 1. Health Check

```bash
curl http://localhost:8080/health
```

### 2. Plugin Health

```bash
curl http://localhost:8080/api/v1/plugins/health | jq
```

### 3. Send Transaction

```bash
curl -X POST http://localhost:8080/api/v1/deliver \
  -H "Content-Type: application/json" \
  -d '{
    "to": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
    "value": 1000000000000000000,
    "gas_limit": 21000,
    "chain_id": 1,
    "priority": "normal",
    "order_id": "manual_test_001",
    "user": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
  }'
```

### 4. Check Transaction Status

```bash
# Replace TX_HASH with actual hash from step 3
curl "http://localhost:8080/api/v1/delivery/TX_HASH/status?chain_id=1" | jq
```

## 🛠 Troubleshooting

### Service Won't Start

1. **Check if Anvil is running**:

   ```bash
   curl -X POST http://localhost:8545 -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
   ```

2. **Restart Anvil**:

   ```bash
   ./setup_local_test.sh anvil-only
   ```

3. **Check environment variables**:
   ```bash
   cat .env
   source .env
   echo $ETH_PRIVATE_KEY
   ```

### Plugin Health Issues

1. **Check plugin health details**:

   ```bash
   curl http://localhost:8080/api/v1/plugins/health | jq '.plugins'
   ```

2. **Check Anvil logs**:

   ```bash
   tail -f anvil.log
   ```

3. **Verify network connectivity**:
   ```bash
   curl -X POST http://localhost:8545 -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"net_version","params":[],"id":1}'
   ```

### Transaction Failures

1. **Check gas limit** - Must be at least 21000 for ETH transfers
2. **Check address format** - Must be valid Ethereum addresses
3. **Check chain ID** - Must be 1 (matching Anvil)
4. **Check account balance** - Anvil accounts start with 10,000 ETH

## 📊 Monitoring

### Real-time Monitoring

```bash
# Terminal 1: Run the service
./setup_local_test.sh

# Terminal 2: Monitor API health
./test_api.sh monitor

# Terminal 3: Watch Anvil logs
tail -f anvil.log
```

### Check Account Balances

```bash
# Check sender balance
curl -X POST http://localhost:8545 -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "latest"],"id":1}'

# Check receiver balance
curl -X POST http://localhost:8545 -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x70997970C51812dc3A010C7d01b50e0d17dc79C8", "latest"],"id":1}'
```

## 🔄 Development Workflow

1. **Start the environment**:

   ```bash
   ./setup_local_test.sh
   ```

2. **Make code changes** to the solver service

3. **Rebuild and restart**:

   ```bash
   # Kill the running service (Ctrl+C)
   ./setup_local_test.sh build-only
   cargo run --release --bin solver-service
   ```

4. **Test changes**:
   ```bash
   ./test_api.sh
   ```

## 🧹 Cleanup

To stop everything and clean up:

```bash
# The script handles cleanup automatically on Ctrl+C, but you can also:
pkill -f anvil
pkill -f solver-service
rm -f anvil.pid anvil.log
```

## 🎯 What This Demonstrates

This local testing setup showcases:

- ✅ **EVM Ethers Plugin Integration** - Real ethers-rs usage with local node
- ✅ **Transaction Lifecycle** - From submission to confirmation
- ✅ **Plugin Health Monitoring** - Real-time status checking
- ✅ **Multi-Priority Delivery** - Different transaction priorities
- ✅ **Error Handling** - Graceful failure modes
- ✅ **API Design** - RESTful endpoints with proper HTTP codes
- ✅ **Observability** - Detailed logging and metrics
- ✅ **Type Safety** - Full compile-time guarantees

This provides a solid foundation for development and testing of the complete OIF Solver system! 🚀
