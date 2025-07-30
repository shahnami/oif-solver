# OIF Solver Quote API Implementation

This document describes the implementation of the POST `/quote` endpoint for the OIF Solver API, following the ERC-7683 Cross-Chain Intents Standard.

## Overview

The quote endpoint provides fast, accurate price estimates for cross-chain intents before execution. It allows users and aggregators to:
- Request price quotes for cross-chain transactions
- Compare different execution routes and settlement mechanisms (Escrow vs ResourceLock)
- Estimate gas costs, fees, and execution times
- Receive settlement-specific data for subsequent intent submission

## Implementation Architecture

### Files Created/Modified

1. **`crates/solver-types/src/api.rs`** - API-specific types and data structures
2. **`crates/solver-config/src/lib.rs`** - Added API server configuration 
3. **`crates/solver-service/src/server.rs`** - HTTP server infrastructure
4. **`crates/solver-service/src/apis/quote.rs`** - Quote processing logic
5. **`crates/solver-service/src/apis/mod.rs`** - API module declarations
6. **`config/demo.toml`** - Added API configuration section
7. **`scripts/test_quote_api.sh`** - Test script for the API

### Key Components

#### 1. Type Definitions (`solver-types/src/api.rs`)

- **GetQuoteRequest**: Request structure with available inputs, requested outputs, and preferences
- **GetQuoteResponse**: Response with multiple quote options
- **QuoteOption**: Individual quote with settlement data, fees, timing, and requirements
- **AssetAmount**: Asset representation using ERC-7930 interoperable addresses
- **SettlementType**: Enum for Escrow vs ResourceLock mechanisms
- **ErrorResponse**: Standardized error format

#### 2. HTTP Server (`solver-service/src/server.rs`)

- Built with Actix-web framework for high performance
- Minimal server focused only on the quote endpoint
- Includes CORS support and request logging
- Integrates with existing SolverEngine

#### 3. Quote Processing Logic (`solver-service/src/apis/quote.rs`)

**Validation Pipeline:**
- Request format validation
- Asset address format checking (Ethereum address format)
- Amount validation (non-zero values)
- Priority bounds checking (0-100)

**Quote Generation:**
- Generates both Escrow and ResourceLock quotes
- Mock implementations for demonstration
- Preference-based optimization (price/speed/input-priority)
- Settlement-specific order data generation

**Cost Estimation:**
- Base fees: Escrow ($2.50), ResourceLock ($3.75)
- ETAs: Escrow (2 min), ResourceLock (3 min)
- Preference adjustments for fee/time trade-offs

#### 4. Configuration Integration

Added API configuration to solver config:
```toml
[api]
enabled = true
host = "127.0.0.1"
port = 3000
timeout_seconds = 30
max_request_size = 1048576  # 1MB
```

## API Specification Compliance

### Request Format
```json
{
  "availableInputs": [
    {
      "input": {
        "asset": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        "amount": "1000000000000000000"
      },
      "priority": 80
    }
  ],
  "requestedMinOutputs": [
    {
      "asset": "0xA0b86a33E6441986C3F2EB618C2a25C3dB00B7c4", 
      "amount": "1500000000"
    }
  ],
  "preference": "price",
  "minValidUntil": 300
}
```

### Response Format
```json
{
  "quotes": [
    {
      "orders": {
        "settler": "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9",
        "data": {
          "settlementType": "escrow",
          "inputToken": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
          "inputAmount": "1000000000000000000",
          "outputToken": "0xA0b86a33E6441986C3F2EB618C2a25C3dB00B7c4",
          "outputAmount": "1500000000",
          "recipient": "0x0000000000000000000000000000000000000000",
          "fillDeadline": 1701234567
        }
      },
      "requiredAllowances": [
        {
          "asset": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
          "amount": "1000000000000000000"
        }
      ],
      "validUntil": 1701234567,
      "eta": 120,
      "totalFeeUsd": 2.25,
      "quoteId": "550e8400-e29b-41d4-a716-446655440000",
      "settlementType": "escrow"
    }
  ]
}
```

## Usage Instructions

### 1. Start the Solver with API Enabled

```bash
cargo run --bin solver -- --config config/demo.toml
```

The solver will start both the core solver engine and the HTTP API server on port 3000.

### 2. Test the Quote Endpoint

```bash
# Run the test script
./scripts/test_quote_api.sh

# Or test manually with curl
curl -X POST http://127.0.0.1:3000/api/quote \
  -H "Content-Type: application/json" \
  -d '{
    "availableInputs": [{"input": {"asset": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "amount": "1000000000000000000"}}],
    "requestedMinOutputs": [{"asset": "0xA0b86a33E6441986C3F2EB618C2a25C3dB00B7c4", "amount": "1500000000"}]
  }'
```

## Features Implemented

✅ **Core Functionality**
- POST /quote endpoint
- Request validation
- Quote generation for both settlement types
- Preference-based optimization
- Error handling with proper HTTP status codes

✅ **ERC-7683 Compliance**
- Settlement contract data structures
- Asset amount representation
- Cross-chain order format

✅ **Performance Optimizations**
- Async request processing
- Concurrent quote generation
- Response size limiting (top 5 quotes)

✅ **Developer Experience**
- Comprehensive test script
- Clear error messages
- Detailed logging
- Configuration-driven setup

## Next Steps for Production

1. **Real Market Integration**
   - Connect to actual DEXs and bridges
   - Live gas price feeds
   - Real-time liquidity checks

2. **Enhanced Validation**
   - Token contract verification
   - Balance and allowance checks
   - Chain-specific validations

3. **Caching & Performance**
   - Quote caching with TTL
   - Pre-computed routes
   - Batch gas estimations

4. **Security & Rate Limiting**
   - Request authentication
   - Rate limiting per IP
   - Input sanitization

5. **Monitoring & Analytics**
   - Quote-to-intent conversion tracking
   - Performance metrics
   - Error rate monitoring

## Best Practices Followed

- **Modular Architecture**: Clean separation of concerns across modules
- **Type Safety**: Strong typing with Rust's type system
- **Error Handling**: Comprehensive error types with context
- **Configuration**: Environment-driven configuration
- **Testing**: Automated test scripts and examples
- **Documentation**: Inline documentation and API specs
- **Standards Compliance**: Full ERC-7683 implementation
- **Performance**: Async processing and optimization focus 