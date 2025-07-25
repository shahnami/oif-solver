# Solver Core Storage Documentation

## Overview

The solver-core uses a key-value storage system with namespaced tables to manage the lifecycle of orders, transactions, and settlements. All storage operations use the `StorageService` which provides serialization/deserialization of data structures.

## Storage Tables

### 1. Orders Table (`orders`)

**Key Format**: `orders:{order_id}`

**Value Type**: `Order` (from solver-order crate)

**Structure**:

```rust
Order {
    id: String,                    // Unique order identifier
    standard: String,              // Protocol standard (e.g., "eip7683")
    created_at: u64,              // Unix timestamp
    data: serde_json::Value,      // Protocol-specific order data
}
```

**Usage**:

- Stored when an intent is validated and converted to an order
- Retrieved during claim processing to generate claim transactions
- Long-term storage for order history and analytics

### 2. Fills Table (`fills`)

**Key Format**: `fills:{order_id}`

**Value Type**: `TransactionHash` (from solver-delivery crate)

**Structure**:

```rust
TransactionHash(Vec<u8>)  // Transaction hash as bytes
```

**Usage**:

- Maps order IDs to their fill transaction hashes
- Stored immediately after submitting a fill transaction
- Used to track which orders have been filled

### 3. Fill Proofs Table (`fill_proofs`)

**Key Format**: `fill_proofs:{order_id}`

**Value Type**: `FillProof` (from solver-order crate)

**Structure**:

```rust
FillProof {
    tx_hash: TransactionHash,           // Fill transaction hash
    block_number: u64,                  // Block where fill was confirmed
    attestation_data: Option<Vec<u8>>,  // Optional attestation/proof data
}
```

**Usage**:

- Stored after monitoring confirms a fill transaction
- Retrieved during claim batch processing
- Contains all data needed to claim rewards/fees

### 4. Claims Table (`claims`)

**Key Format**: `claims:{order_id}`

**Value Type**: `TransactionHash` (from solver-delivery crate)

**Structure**:

```rust
TransactionHash(Vec<u8>)  // Claim transaction hash as bytes
```

**Usage**:

- Maps order IDs to their claim transaction hashes
- Stored after submitting a claim transaction
- Used to track which orders have been claimed

## Data Flow

1. **Intent Discovery → Order Storage**

   ```
   Intent validated → Store in "orders" table
   ```

2. **Order Execution → Fill Tracking**

   ```
   Fill transaction submitted → Store tx_hash in "fills" table
   ```

3. **Fill Monitoring → Proof Generation**

   ```
   Fill confirmed → Generate FillProof → Store in "fill_proofs" table
   ```

4. **Claim Processing → Settlement Tracking**
   ```
   Claim transaction submitted → Store tx_hash in "claims" table
   ```

## Storage Operations

### Write Operations

- `store("orders", order_id, order)` - Save validated order
- `store("fills", order_id, tx_hash)` - Track fill transaction
- `store("fill_proofs", order_id, proof)` - Save fill confirmation proof
- `store("claims", order_id, tx_hash)` - Track claim transaction

### Read Operations

- `retrieve("orders", order_id)` - Get order details
- `retrieve("fill_proofs", order_id)` - Get fill proof for claiming

## Consistency Considerations

1. **Atomic Operations**: Each storage operation is atomic at the key level
2. **Order Dependencies**: Data must be stored in sequence (order → fill → proof → claim)
3. **Idempotency**: Storage operations should be idempotent to handle retries
4. **TTL Support**: The storage backend supports TTL for automatic cleanup of old data

## Error Handling

All storage operations return `Result<T, StorageError>` where:

- `StorageError::NotFound` - Key doesn't exist
- `StorageError::Serialization` - Data serialization/deserialization failed
- `StorageError::Backend` - Backend-specific errors

## Future Considerations

1. **Indexing**: May need secondary indices for efficient queries
2. **Batch Operations**: Could benefit from batch read/write operations
3. **Transaction Support**: May need multi-key transactions for complex operations
4. **Migration Support**: Schema versioning for data structure evolution
