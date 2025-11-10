# Bitcoin Batch Coordinator

A peer-to-peer Bitcoin transaction batching system that combines individual transactions into coordinated batches. Using Pubky and BDK, we're able to bring together multiple participants to save on transaction fees and improve on-chain efficiency.

## Quick Start

### Prerequisites

Clone & Build:
```bash
git clone https://github.com/coreyphillips/bitcoin-batch-coordinator.git && cd bitcoin-batch-coordinator && cargo build --release
```

### Running a Coordinator

Start a batch coordinator that other can join:

```bash
# Using a recovery file
./target/release/coordinator coordinator.pkarr \
  --pass "password" \
  --network regtest \
  --fee-rate 10 \
  --min 2 \
  --max 5

# Or using a recovery phrase directly
./target/release/coordinator \
  --recovery-phrase "your twelve word mnemonic phrase for pubky messenger identity" \
  --pass "password" \
  --network regtest \
  --fee-rate 10 \
  --min 2 \
  --max 5
```

The coordinator will output its pubky - share this with participants:
```
INFO Coordinator pubky: zxcvbn123abc...
```

### Joining as a Participant

Join a batch using the coordinator's pubky:

*Note: In order to participate in a batch, the coordinator must follow you on [pubky.app](pubky.app).*

```bash
# Using a recovery file for pubky identity
./target/release/participant participant.pkarr COORDINATOR_PUBKEY \
  --pass "password" \
  --mnemonic "your bitcoin wallet twelve word mnemonic phrase here" \
  --pay "tb1qaddress1:50000,tb1qaddress2:25000"

# Or using recovery phrases directly
./target/release/participant COORDINATOR_PUBKEY \
  --pubky-phrase "pubky messenger identity mnemonic phrase" \
  --pass "password" \
  --mnemonic "your bitcoin wallet mnemonic phrase" \
  --pay "tb1qaddress1:50000,tb1qaddress2:25000"
```

That's it! The participant will automatically:
- Query the coordinator for the current batch
- Submit inputs and outputs
- Sign the transaction
- Wait for broadcast confirmation

## Advanced Features

### Multi-Batch Mode (Rolling Batches)

Run a coordinator that automatically creates new batches as they fill:

```bash
./target/release/coordinator coordinator.pkarr \
  --pass "password" \
  --network signet \
  --fee-rate 10 \
  --min 2 \
  --max 5 \
  --multi-batch
```

### Manual UTXO Selection

Specify exactly which UTXOs to use:

```bash
./target/release/participant participant.pkarr COORDINATOR_PUBKEY \
  --pass "password" \
  --mnemonic "..." \
  --pay "tb1q...:50000" \
  --select "txid1:0,txid2:1"
```

### Sign and Leave

Exit immediately after signing (useful for automation):

```bash
./target/release/participant participant.pkarr COORDINATOR_PUBKEY \
  --pass "password" \
  --mnemonic "..." \
  --pay "tb1q...:50000" \
  --no-wait
```

### Using Different BIP84 Accounts

Specify a different BIP84 account number (default is 0):

```bash
./target/release/participant participant.pkarr COORDINATOR_PUBKEY \
  --pass "password" \
  --mnemonic "..." \
  --pay "tb1q...:50000" \
  --account 1
```

### Coordinator with Expected Participants

Run a coordinator that only accepts specific participants:

```bash
./target/release/coordinator coordinator.pkarr \
  --pass "password" \
  --network signet \
  --fee-rate 10 \
  --min 2 \
  --max 5 \
  --participants "pkarr1,pkarr2,pkarr3"
```

### Seeding Follow List

The coordinator can automatically seed its follow list with specified pubkys and recursively follow the users they follow. This helps bootstrap the coordinator's network by discovering participants through the social graph:

```bash
./target/release/coordinator coordinator.pkarr \
  --pass "password" \
  --network signet \
  --fee-rate 10 \
  --min 2 \
  --max 5 \
  --seed-follows "pubky1,pubky2,pubky3"
```

When using `--seed-follows`:
- The coordinator will follow each specified pubky
- It will also discover and follow the users that these seed pubkeys follow
- This creates a recursive discovery mechanism through the follow graph
- Duplicate follows are automatically prevented (won't re-follow already followed users)
- The coordinator will never follow itself, even if referenced by a seed pubky

This is particularly useful for:
- Bootstrapping new coordinators with an initial participant network
- Discovering participants through trusted seed accounts
- Building a participant pool based on social connections

### Broadcasting to Followers

By default, the coordinator will NOT automatically broadcast batch intents to all discovered followers. This prevents spamming potentially thousands of users with unsolicited messages. To enable broadcasting to followers:

```bash
./target/release/coordinator coordinator.pkarr \
  --pass "password" \
  --network signet \
  --fee-rate 10 \
  --min 2 \
  --max 5 \
  --broadcast-to-followers
```

When `--broadcast-to-followers` is enabled:
- The coordinator will send the current batch intent to all discovered peers
- This can help notify potential participants about available batches
- Use with caution if you have many followers to avoid message spam

Without this flag (default behavior):
- Participants must explicitly query the coordinator for current batches
- The coordinator only responds to direct requests
- This is more scalable and avoids unwanted notifications

### Custom Electrum Server

Both coordinator and participant support custom Electrum servers:

```bash
# Add to either command:
--electrum-host electrum.blockstream.info \
--electrum-port 60602 \
--electrum-proto ssl
```

**Note:** The system automatically selects appropriate Electrum servers based on the network:
- **Bitcoin Mainnet:** blockstream.info:700 (ssl)
- **Testnet:** blockstream.info:60001 (ssl)
- **Signet:** mempool.space:60602 (ssl)
- **Regtest:** localhost:50001 (tcp)

## How It Works

1. **Coordinator** announces a batch with parameters (fee rate, min/max participants, deadline)
2. **Participants** commit to their inputs/outputs with a hash
3. **Participants** reveal their actual inputs/outputs
4. **Coordinator** builds a deterministic transaction template
5. **Participants** sign their inputs using PSBT
6. **Coordinator** combines signatures and broadcasts

## Networks Supported

- `bitcoin` - Bitcoin Mainnet
- `testnet` - Bitcoin Testnet3
- `signet` - Bitcoin Signet
- `regtest` - Bitcoin Regtest

## Security Features

- **Commitment-Reveal**: Prevents front-running and censorship
- **PSBT Signing**: Private keys never leave your device
- **Deterministic Assembly**: All participants can verify the transaction
- **Timeout Protection**: Automatic fallback if participants don't respond
- **Ban System**: Malicious participants are automatically banned

## Testing

Run the integration test script:

```bash
./examples/run-batch-test.sh
```

This will:
1. Start a coordinator
2. Run multiple participants
3. Complete a full batch transaction
4. Show detailed logs of the entire process


## Troubleshooting

**"Timeout waiting for current_intent"**
- Coordinator may not be running or network issues
- Check coordinator logs for your participant's connection

**"Insufficient participants by deadline"**
- Not enough participants joined before timeout
- Try increasing `--deadline-ms` or lowering `--min`

**"Invalid commitment - reveal doesn't match hash"**
- Internal error, please report as bug
- May indicate network corruption

**"UTXO not found"**
- The UTXO doesn't exist or was already spent
- Check transaction ID and output index

## Protocol Specification

### Message Flow

```
Participant                    Coordinator
    |                               |
    |-------- GetCurrentIntent ----->|
    |<-------- CurrentIntent --------|
    |                               |
    |---------- Commitment --------->|
    |<------------- Ack -------------|
    |                               |
    |<-------- RequestReveal --------|
    |------------ Reveal ----------->|
    |<------------- Ack -------------|
    |                               |
    |<---------- Template -----------|
    |---------- SigFragment -------->|
    |                               |
    |<---------- FinalTx ------------|
```

### Transaction Rules

- Inputs sorted by: `(txid_bytes, vout)`
- Outputs sorted by: `(script_bytes, value)`
- Version: 2
- Locktime: 0
- Sequence: 0xFFFFFFFF
- Signatures: SIGHASH_ALL only
- Minimum output: 546 satoshis (dust limit)

## Using as a Library

This project can be used as a library in other Rust projects, not just as a CLI tool. We provide simple, high-level APIs that are as easy to use as the CLI.

### Add to your Cargo.toml

```toml
[dependencies]
coordinator = { git = "https://https://github.com/coreyphillips/bitcoin-batch-coordinator", package = "coordinator" }
participant = { git = "https://https://github.com/coreyphillips/bitcoin-batch-coordinator", package = "participant" }
```

### Simple Coordinator Example

```rust
use coordinator::run_coordinator_simple;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Run a coordinator with just the essentials
    run_coordinator_simple(
        "your twelve word mnemonic phrase",
        "password",
        "signet",
        10,  // fee rate
        2,   // min participants
        5,   // max participants
    ).await?;
    Ok(())
}
```

### Simple Participant Example

```rust
use participant::run_participant_simple;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Join a batch with minimal configuration
    run_participant_simple(
        "coordinator_pubky_here",
        "your pubky mnemonic",
        "password",
        "your bitcoin wallet mnemonic",
        "signet",
        vec![
            ("tb1qaddress1", 50000),
            ("tb1qaddress2", 25000),
        ],
    ).await?;
    Ok(())
}
```

### Advanced Configuration

For more control, use the config structs:

```rust
use coordinator::{run_coordinator, CoordinatorConfig};

let config = CoordinatorConfig {
    network: "signet".to_string(),
    fee_rate: 10,
    min_participants: 2,
    max_participants: 10,
    multi_batch: true,  // Enable rolling batches
    broadcast_to_followers: false,
    ..Default::default()
};

run_coordinator(config).await?;
```

See `examples/library_usage.rs` for more examples of library integration.

## Building from Source

```bash
# Clone the repository
git clone https://https://github.com/coreyphillips/bitcoin-batch-coordinator
cd https://github.com/coreyphillips/bitcoin-batch-coordinator

# Build release binaries
cargo build --release

# Run tests
cargo test --all

# Check code
cargo clippy --all
cargo fmt --all -- --check
```

## License

MIT

## Contributing

Contributions welcome! Please ensure:
- All tests pass: `cargo test --all`
- Code is formatted: `cargo fmt --all`
- No clippy warnings: `cargo clippy --all`

## Support

For issues or questions:
- Open an issue on GitHub
- Join our Discord/Telegram
- Email: support@example.com