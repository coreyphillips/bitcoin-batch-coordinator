# Examples

This directory contains ready-to-run examples for testing the Bitcoin Batch Coordinator on regtest.

## Quick Start

All examples use pre-configured test credentials that you can copy and paste directly.

### Test Configuration

The examples use these test credentials:

**Coordinator:**
- Pkarr file: `examples/coordinator.pkarr`
- Password: `password`
- Pubky: `39ruj459yauxy5g1n4gts1hn5wd35q3d1yffxckia6zsa6x1gtsy`

**Participant 1:**
- Pkarr file: `examples/p1.pkarr`
- Password: `password`
- Bitcoin mnemonic: `zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong`
- Receive address: `bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2`

**Participant 2:**
- Pkarr file: `examples/p2.pkarr`
- Password: `password`
- Bitcoin mnemonic: `test test test test test test test test test test test junk`
- Receive address: `bcrt1q6rhpng9evdsfnn833a4f4vej0asu6j8q8s7su`

**Regtest Electrum Server (Bitkit Development):**
- Host: `34.65.252.32`
- TCP Port: `18483`
- SSL Port: `18484`

## Automated Test

The easiest way to test is using the automated test script:

```bash
./examples/run-batch-test.sh
```

This script will:
1. Build the project
2. Start the coordinator
3. Start two participants
4. Monitor the batch process
5. Verify the transaction on-chain

## Manual Test (3 Terminals)

If you want to run each component manually to see the output:

### Terminal 1: Coordinator

```bash
RUST_LOG=info cargo run --release -p coordinator -- \
  examples/coordinator.pkarr \
  --pass "password" \
  --network regtest \
  --fee-rate 1 \
  --min 2 \
  --max 2 \
  --deadline-ms 120000 \
  --electrum-host 34.65.252.32 \
  --electrum-port 18483 \
  --electrum-proto tcp
```


### Terminal 2: Participant 1

```bash
RUST_LOG=info cargo run --release -p participant -- \
  examples/p1.pkarr \
  39ruj459yauxy5g1n4gts1hn5wd35q3d1yffxckia6zsa6x1gtsy \
  --pass "password" \
  --mnemonic "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong" \
  --network regtest \
  --pay "bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:1000,bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:1000,bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:1000" \
  --electrum-host 34.65.252.32 \
  --electrum-port 18483 \
  --electrum-proto tcp \
  --no-wait
```

### Terminal 3: Participant 2

```bash
RUST_LOG=info cargo run --release -p participant -- \
  examples/p2.pkarr \
  39ruj459yauxy5g1n4gts1hn5wd35q3d1yffxckia6zsa6x1gtsy \
  --pass "password" \
  --mnemonic "test test test test test test test test test test test junk" \
  --network regtest \
  --pay "bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:5000" \
  --electrum-host 34.65.252.32 \
  --electrum-port 18483 \
  --electrum-proto tcp \
  --no-wait
```

## What Happens in a Batch

1. **Coordinator starts** and creates a batch intent
2. **Participants join** by committing to their payments
3. **Coordinator collects commitments** (phase 1)
4. **Participants reveal** their inputs and payment details (phase 2)
5. **Coordinator builds** the transaction deterministically
6. **Participants sign** their inputs (phase 3)
7. **Coordinator broadcasts** the final transaction
8. **Transaction confirmed** on the blockchain

## Troubleshooting

### "Failed to connect to Electrum"
- Check that the Electrum server is running and accessible
- Verify firewall settings
- Try a different Electrum server

### "Insufficient funds"
- Ensure test wallets are funded (see "Funding Test Wallets" above)
- Check wallet balance with the participant's receive address

### "Timeout waiting for participants"
- Increase `--deadline-ms` on the coordinator
- Check that all participants are connecting to the same coordinator pubky
- Verify the Intent ID matches across all participants

### Logs

When using the automated test script, logs are saved to:
- `examples/logs/coordinator.log` - Coordinator output
- `examples/logs/participant1.log` - Participant 1 output
- `examples/logs/participant2.log` - Participant 2 output

Check these logs for detailed error messages.

## Files in This Directory

- `coordinator.pkarr` - Pre-configured coordinator identity
- `p1.pkarr` - Pre-configured participant 1 identity
- `p2.pkarr` - Pre-configured participant 2 identity
- `run-batch-test.sh` - Automated test script (older, instructional version)
- `logs/` - Output logs from test runs (created automatically)