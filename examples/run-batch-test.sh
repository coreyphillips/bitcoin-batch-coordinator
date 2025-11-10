#!/bin/bash

# Bitcoin Batch Coordinator Reliable Test Runner
# This script provides a more reliable way to test the batch coordination process

set -e

# Determine script directory and project root
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$( cd "$SCRIPT_DIR/.." && pwd )"

# Change to project root to ensure cargo commands work
cd "$PROJECT_ROOT"

# Determine pkarr file paths (always relative to project root)
PKARR_DIR="examples"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default configuration
RUST_LOG="${RUST_LOG:-info}"
BUILD_MODE="${BUILD_MODE:-release}"
TIMEOUT="${TIMEOUT:-300}" # 5 minutes default timeout
COORDINATOR_KEY="39ruj459yauxy5g1n4gts1hn5wd35q3d1yffxckia6zsa6x1gtsy"
NETWORK="${NETWORK:-regtest}" # Default to regtest

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --debug)
            BUILD_MODE="debug"
            RUST_LOG="debug"
            shift
            ;;
        --no-build)
            SKIP_BUILD=1
            shift
            ;;
        --network)
            NETWORK="$2"
            shift 2
            ;;
        --timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [options]"
            echo "Options:"
            echo "  --debug          Run in debug mode with verbose logging"
            echo "  --no-build       Skip the build step"
            echo "  --network NET    Set network: regtest, testnet, signet, bitcoin (default: regtest)"
            echo "  --timeout SEC    Set timeout in seconds (default: 300)"
            echo "  --help           Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Set network-specific Electrum defaults
case $NETWORK in
    bitcoin|mainnet)
        NETWORK="bitcoin"
        # Mainnet defaults
        ELECTRUM_HOST="${ELECTRUM_HOST:-35.187.18.233}"
        ELECTRUM_PORT="${ELECTRUM_PORT:-8900}"
        ELECTRUM_PROTO="${ELECTRUM_PROTO:-ssl}"
        ;;
    testnet)
        # Testnet defaults (you can customize these)
        ELECTRUM_HOST="${ELECTRUM_HOST:-electrum.blockstream.info}"
        ELECTRUM_PORT="${ELECTRUM_PORT:-60002}"
        ELECTRUM_PROTO="${ELECTRUM_PROTO:-ssl}"
        ;;
    signet)
        # Signet defaults (you can customize these)
        ELECTRUM_HOST="${ELECTRUM_HOST:-electrum.blockstream.info}"
        ELECTRUM_PORT="${ELECTRUM_PORT:-60602}"
        ELECTRUM_PROTO="${ELECTRUM_PROTO:-ssl}"
        ;;
    regtest|*)
        NETWORK="regtest"
        # Regtest defaults
        ELECTRUM_HOST="${ELECTRUM_HOST:-34.65.252.32}"
        ELECTRUM_PORT="${ELECTRUM_PORT:-18483}"
        ELECTRUM_PROTO="${ELECTRUM_PROTO:-tcp}"
        ;;
esac

echo -e "${GREEN}🚀 Bitcoin Batch Coordinator Reliable Test Runner${NC}"
echo "================================================"
echo -e "${BLUE}Configuration:${NC}"
echo "  - Network: $NETWORK"
echo "  - Electrum: $ELECTRUM_PROTO://$ELECTRUM_HOST:$ELECTRUM_PORT"
echo "  - Build mode: $BUILD_MODE"
echo "  - Log level: $RUST_LOG"
echo "  - Timeout: ${TIMEOUT}s"
echo "================================================"

# Create logs directory
mkdir -p logs

# Build if not skipped
if [ -z "$SKIP_BUILD" ]; then
    echo -e "${YELLOW}Building in $BUILD_MODE mode...${NC}"
    if [ "$BUILD_MODE" = "release" ]; then
        cargo build --release -p coordinator -p participant 2>&1 | grep -v warning || true
    else
        cargo build -p coordinator -p participant 2>&1 | grep -v warning || true
    fi
    echo -e "${GREEN}✓ Build complete${NC}"
fi

# Cleanup function
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"

    # Kill all background processes
    if [ -n "$COORDINATOR_PID" ]; then
        kill $COORDINATOR_PID 2>/dev/null || true
    fi
    if [ -n "$PARTICIPANT1_PID" ]; then
        kill $PARTICIPANT1_PID 2>/dev/null || true
    fi
    if [ -n "$PARTICIPANT2_PID" ]; then
        kill $PARTICIPANT2_PID 2>/dev/null || true
    fi

    # Kill any monitoring processes
    if [ -n "$MONITOR_PID" ]; then
        kill $MONITOR_PID 2>/dev/null || true
    fi

    echo -e "${GREEN}✓ Cleanup complete${NC}"
}

# Set trap for cleanup on exit
trap cleanup EXIT

# Function to wait for a specific log message with timeout
wait_for_log_message() {
    local log_file=$1
    local search_pattern=$2
    local timeout_sec=$3
    local description=$4

    echo -e "${YELLOW}Waiting for: $description${NC}"

    local elapsed=0
    while [ $elapsed -lt $timeout_sec ]; do
        if grep -q "$search_pattern" "$log_file" 2>/dev/null; then
            echo -e "${GREEN}✓ Found: $description${NC}"
            return 0
        fi
        sleep 0.5
        elapsed=$((elapsed + 1))
    done

    echo -e "${RED}✗ Timeout waiting for: $description${NC}"
    return 1
}

# Start coordinator
echo -e "\n${GREEN}1. Starting Coordinator${NC}"
echo "================================================"

COORDINATOR_LOG="logs/coordinator-$(date +%Y%m%d-%H%M%S).log"
PARTICIPANT1_LOG="logs/participant1-$(date +%Y%m%d-%H%M%S).log"
PARTICIPANT2_LOG="logs/participant2-$(date +%Y%m%d-%H%M%S).log"

# Create symlinks to latest logs for easy access
ln -sf "$(basename $COORDINATOR_LOG)" logs/coordinator.log
ln -sf "$(basename $PARTICIPANT1_LOG)" logs/participant1.log
ln -sf "$(basename $PARTICIPANT2_LOG)" logs/participant2.log

echo "Log files:"
echo "  - Coordinator: $COORDINATOR_LOG"
echo "  - Participant 1: $PARTICIPANT1_LOG"
echo "  - Participant 2: $PARTICIPANT2_LOG"

# Start coordinator in background
RUST_LOG=$RUST_LOG cargo run --$BUILD_MODE -p coordinator -- \
    $PKARR_DIR/coordinator.pkarr \
    --pass "password" \
    --network $NETWORK \
    --fee-rate 1 \
    --min 2 \
    --max 2 \
    --deadline-ms 120000 \
    --electrum-host $ELECTRUM_HOST \
    --electrum-port $ELECTRUM_PORT \
    --electrum-proto $ELECTRUM_PROTO \
    > "$COORDINATOR_LOG" 2>&1 &
COORDINATOR_PID=$!

echo "Coordinator PID: $COORDINATOR_PID"

# Wait for coordinator to start and get intent ID
if ! wait_for_log_message "$COORDINATOR_LOG" "Intent ID:" 10 "Coordinator startup"; then
    echo -e "${RED}Failed to start coordinator${NC}"
    tail -20 "$COORDINATOR_LOG"
    exit 1
fi

# Extract intent ID
INTENT_ID=$(grep "Intent ID:" "$COORDINATOR_LOG" | tail -1 | awk '{print $NF}')
echo -e "${GREEN}✓ Intent ID: $INTENT_ID${NC}"

# Define payment specifications for later verification
PARTICIPANT1_PAYMENTS="bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:1000,bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:1000,bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:1000"
PARTICIPANT2_PAYMENTS="bcrt1qqlqf49q2lncatcwtkxce9033n9s5g2y3kl5ax2:5000"

# Start participant 1
echo -e "\n${GREEN}2. Starting Participant 1${NC}"
echo "================================================"

RUST_LOG=$RUST_LOG cargo run --$BUILD_MODE -p participant -- \
    $PKARR_DIR/p1.pkarr \
    $COORDINATOR_KEY \
    --pass "password" \
    --mnemonic "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong" \
    --network $NETWORK \
    --pay "$PARTICIPANT1_PAYMENTS" \
    --intent-id "$INTENT_ID" \
    --electrum-host $ELECTRUM_HOST \
    --electrum-port $ELECTRUM_PORT \
    --electrum-proto $ELECTRUM_PROTO \
    --no-wait \
    > "$PARTICIPANT1_LOG" 2>&1 &
PARTICIPANT1_PID=$!

echo "Participant 1 PID: $PARTICIPANT1_PID"

# Give participant 1 a moment to start
sleep 2

# Start participant 2
echo -e "\n${GREEN}3. Starting Participant 2${NC}"
echo "================================================"

RUST_LOG=$RUST_LOG cargo run --$BUILD_MODE -p participant -- \
    $PKARR_DIR/p2.pkarr \
    $COORDINATOR_KEY \
    --pass "password" \
    --mnemonic "test test test test test test test test test test test junk" \
    --network $NETWORK \
    --pay "$PARTICIPANT2_PAYMENTS" \
    --intent-id "$INTENT_ID" \
    --electrum-host $ELECTRUM_HOST \
    --electrum-port $ELECTRUM_PORT \
    --electrum-proto $ELECTRUM_PROTO \
    --no-wait \
    > "$PARTICIPANT2_LOG" 2>&1 &
PARTICIPANT2_PID=$!

echo "Participant 2 PID: $PARTICIPANT2_PID"

# Monitor the batch process
echo -e "\n${GREEN}4. Monitoring Batch Process${NC}"
echo "================================================"

# Function to show progress
show_progress() {
    local current_time=$(date +%s)
    local start_time=$1
    local elapsed=$((current_time - start_time))

    # Check coordinator progress
    local phase="Starting"

    if grep -q "Phase 1: Collecting commitments" "$COORDINATOR_LOG" 2>/dev/null; then
        phase="Phase 1: Commitments"

        local commitment_count=$(grep -c "✅ Received commitment" "$COORDINATOR_LOG" 2>/dev/null || echo 0)
        phase="$phase ($commitment_count/2)"
    fi

    if grep -q "Phase 2: Requesting reveals" "$COORDINATOR_LOG" 2>/dev/null; then
        phase="Phase 2: Reveals"

        local reveal_count=$(grep -c "Verified reveal from" "$COORDINATOR_LOG" 2>/dev/null || echo 0)
        phase="$phase ($reveal_count/2)"
    fi

    if grep -q "Building deterministic transaction" "$COORDINATOR_LOG" 2>/dev/null; then
        phase="Building transaction"
    fi

    if grep -q "Waiting for signature fragments" "$COORDINATOR_LOG" 2>/dev/null; then
        phase="Collecting signatures"

        local sig_count=$(grep -c "Received signature fragment" "$COORDINATOR_LOG" 2>/dev/null || echo 0)
        phase="$phase ($sig_count/2)"
    fi

    if grep -q "Broadcasting transaction" "$COORDINATOR_LOG" 2>/dev/null; then
        phase="Broadcasting"
    fi

    printf "\r[%3ds] Status: %-40s" "$elapsed" "$phase"
}

# Start monitoring
START_TIME=$(date +%s)
SUCCESS=0

# Monitor with timeout
while true; do
    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - START_TIME))

    # Show progress
    show_progress $START_TIME

    # Check for success
    if grep -q "Batch coordination complete!" "$COORDINATOR_LOG" 2>/dev/null; then
        SUCCESS=1
        break
    fi

    # Check for failure conditions
    if grep -q "Insufficient participants" "$COORDINATOR_LOG" 2>/dev/null; then
        echo -e "\n${RED}✗ Failed: Insufficient participants${NC}"
        break
    fi

    if grep -q "Failed to broadcast" "$COORDINATOR_LOG" 2>/dev/null; then
        echo -e "\n${RED}✗ Failed: Transaction broadcast failed${NC}"
        break
    fi

    # Check if coordinator died
    if ! kill -0 $COORDINATOR_PID 2>/dev/null; then
        echo -e "\n${YELLOW}Coordinator process ended${NC}"
        break
    fi

    # Check timeout
    if [ $ELAPSED -gt $TIMEOUT ]; then
        echo -e "\n${RED}✗ Timeout reached (${TIMEOUT}s)${NC}"
        break
    fi

    sleep 0.5
done

echo # New line after progress

# Show results
echo -e "\n${GREEN}5. Results${NC}"
echo "================================================"

if [ $SUCCESS -eq 1 ]; then
    TXID=$(grep "TXID:" "$COORDINATOR_LOG" | tail -1 | awk '{print $NF}')
    PARTICIPANTS=$(grep "Participants:" "$COORDINATOR_LOG" | tail -1 | awk '{print $NF}')

    echo -e "${GREEN}✅ SUCCESS!${NC}"
    echo -e "  Transaction ID: ${GREEN}$TXID${NC}"
    echo -e "  Participants: $PARTICIPANTS"
    echo -e "  Duration: ${ELAPSED}s"

    # Show key milestones
    echo -e "\n${BLUE}Milestones:${NC}"
    grep -E "Received commitment from|Verified reveal from|Received signature fragment|Transaction broadcast successful" "$COORDINATOR_LOG" | while read -r line; do
        timestamp=$(echo "$line" | cut -d' ' -f1)
        message=$(echo "$line" | cut -d' ' -f4-)
        echo "  $timestamp - $message"
    done

    # Verify transaction on chain
    echo -e "\n${GREEN}6. Transaction Verification${NC}"
    echo "================================================"

    # Create a temporary Rust program to check the transaction
    VERIFY_DIR=$(mktemp -d)
    mkdir -p "$VERIFY_DIR/src"
    cat > "$VERIFY_DIR/Cargo.toml" << 'EOF'
[package]
name = "tx-verify"
version = "0.1.0"
edition = "2021"

[dependencies]
bdk = { version = "0.30", features = ["electrum"] }
hex = "0.4"
EOF

    # Determine the BDK network enum value
    case $NETWORK in
        bitcoin) BDK_NETWORK="Bitcoin" ;;
        testnet) BDK_NETWORK="Testnet" ;;
        signet) BDK_NETWORK="Signet" ;;
        regtest) BDK_NETWORK="Regtest" ;;
        *) BDK_NETWORK="Regtest" ;;
    esac

    cat > "$VERIFY_DIR/src/main.rs" << EOF
use bdk::electrum_client::{Client as ElectrumClient, ElectrumApi};
use bdk::bitcoin::{Txid, Address, Network};
use std::str::FromStr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let txid_str = "$TXID";
    let txid = Txid::from_str(txid_str)?;

    // Connect to Electrum
    let electrum_url = "$ELECTRUM_PROTO://$ELECTRUM_HOST:$ELECTRUM_PORT";
    let client = ElectrumClient::new(electrum_url)?;
    let height = client.block_headers_subscribe()?.height;

    // Set correct network for address parsing
    let network = Network::$BDK_NETWORK;

    // Fetch the transaction
    match client.transaction_get(&txid) {
        Ok(tx) => {
            println!("TRANSACTION_FOUND=true");
            println!("VERSION={}", tx.version);
            println!("SIZE={}", bdk::bitcoin::consensus::encode::serialize(&tx).len());
            println!("VSIZE={}", tx.vsize());
            println!("WEIGHT={}", tx.weight());
            println!("NUM_INPUTS={}", tx.input.len());
            println!("NUM_OUTPUTS={}", tx.output.len());

            // Calculate fees
            let mut total_input_value = 0u64;
            let total_output_value: u64 = tx.output.iter().map(|o| o.value).sum();

            for input in tx.input.iter() {
                if let Ok(prev_tx) = client.transaction_get(&input.previous_output.txid) {
                    if let Some(prev_out) = prev_tx.output.get(input.previous_output.vout as usize) {
                        total_input_value += prev_out.value;
                    }
                }
            }

            if total_input_value > 0 {
                let fee = total_input_value - total_output_value;
                let fee_rate = (fee as f64) / (tx.vsize() as f64);
                println!("TOTAL_INPUT={}", total_input_value);
                println!("TOTAL_OUTPUT={}", total_output_value);
                println!("FEE={}", fee);
                println!("FEE_RATE={:.2}", fee_rate);
            }

            // Check confirmation status
            match client.transaction_get_merkle(&txid, height) {
                Ok(proof) if proof.block_height > 0 => {
                    let confirmations = height - proof.block_height + 1;
                    println!("CONFIRMATIONS={}", confirmations);
                    println!("BLOCK_HEIGHT={}", proof.block_height);
                }
                _ => {
                    println!("CONFIRMATIONS=0");
                    println!("STATUS=mempool");
                }
            }

            // Count payment vs change outputs
            let payment_count = tx.output.iter().filter(|o| o.value == 50000).count();
            let change_count = tx.output.iter().filter(|o| o.value != 50000).count();
            println!("PAYMENT_OUTPUTS={}", payment_count);
            println!("CHANGE_OUTPUTS={}", change_count);

            // Show output addresses
            for (idx, output) in tx.output.iter().enumerate() {
                if let Ok(addr) = Address::from_script(&output.script_pubkey, network) {
                    println!("OUTPUT_{}={}:{}", idx, addr, output.value);
                }
            }
        }
        Err(e) => {
            println!("TRANSACTION_FOUND=false");
            println!("ERROR={}", e);
        }
    }

    Ok(())
}
EOF

    # Build and run the verification program
    echo -e "${YELLOW}Fetching transaction from blockchain...${NC}"

    if cd "$VERIFY_DIR" && cargo build --release --quiet 2>/dev/null && ./target/release/tx-verify > verify_output.txt 2>&1; then
        # Parse the output
        source verify_output.txt

        if [ "$TRANSACTION_FOUND" = "true" ]; then
            echo -e "${GREEN}✅ Transaction verified on chain!${NC}"
            echo ""
            echo -e "${BLUE}Transaction Details:${NC}"
            echo "  Size: $SIZE bytes ($VSIZE vB)"
            echo "  Weight: $WEIGHT weight units"
            echo "  Inputs: $NUM_INPUTS"
            echo "  Outputs: $NUM_OUTPUTS"

            if [ -n "$FEE" ]; then
                echo ""
                echo -e "${BLUE}Fee Analysis:${NC}"
                echo "  Total Input:  $TOTAL_INPUT sats"
                echo "  Total Output: $TOTAL_OUTPUT sats"
                echo "  Fee:          $FEE sats"
                echo "  Fee Rate:     $FEE_RATE sat/vB"
            fi

            echo ""
            echo -e "${BLUE}Output Breakdown:${NC}"
            echo "  Payment outputs: $PAYMENT_OUTPUTS (50000 sats each)"
            echo "  Change outputs:  $CHANGE_OUTPUTS"

            if [ "$CONFIRMATIONS" = "0" ]; then
                echo ""
                echo -e "${YELLOW}Status: Unconfirmed (in mempool)${NC}"
            else
                echo ""
                echo -e "${GREEN}Status: Confirmed${NC}"
                echo "  Block Height: $BLOCK_HEIGHT"
                echo "  Confirmations: $CONFIRMATIONS"
            fi

            # Show all outputs
            echo ""
            echo -e "${BLUE}Transaction Outputs:${NC}"

            # Build a list of expected payments from both participants
            ALL_PAYMENTS="$PARTICIPANT1_PAYMENTS,$PARTICIPANT2_PAYMENTS"

            idx=0
            while true; do
                OUTPUT_VAR="OUTPUT_$idx"
                if [ -n "${!OUTPUT_VAR}" ]; then
                    OUTPUT_INFO="${!OUTPUT_VAR}"
                    ADDR=$(echo "$OUTPUT_INFO" | cut -d':' -f1)
                    AMOUNT=$(echo "$OUTPUT_INFO" | cut -d':' -f2)

                    # Check if this output matches any of the expected payments
                    IS_PAYMENT=false
                    IFS=',' read -ra PAYMENTS <<< "$ALL_PAYMENTS"
                    for payment in "${PAYMENTS[@]}"; do
                        if [ "$payment" = "$OUTPUT_INFO" ]; then
                            IS_PAYMENT=true
                            break
                        fi
                    done

                    if [ "$IS_PAYMENT" = "true" ]; then
                        echo "  [$idx] Payment  → $ADDR ($AMOUNT sats)"
                    else
                        echo "  [$idx] Change   → $ADDR ($AMOUNT sats)"
                    fi
                    idx=$((idx + 1))
                else
                    break
                fi
            done

        else
            echo -e "${YELLOW}⚠ Transaction not yet visible on chain${NC}"
            echo "  This is normal - it may take a moment to propagate"
            if [ -n "$ERROR" ]; then
                echo "  Error: $ERROR"
            fi
        fi
    else
        echo -e "${YELLOW}⚠ Could not verify transaction (verification tool build failed)${NC}"
    fi

    # Cleanup verification directory
    rm -rf "$VERIFY_DIR"

    echo ""
    echo -e "${GREEN}✨ Batch coordination completed successfully!${NC}"
    exit 0
else
    echo -e "${RED}✗ FAILED${NC}"
    echo -e "  Duration: ${ELAPSED}s"

    # Show last coordinator state
    echo -e "\n${YELLOW}Last coordinator messages:${NC}"
    tail -10 "$COORDINATOR_LOG" | grep -E "INFO|WARN|ERROR" || tail -10 "$COORDINATOR_LOG"

    # Check participant status
    echo -e "\n${YELLOW}Participant status:${NC}"

    if [ -f "$PARTICIPANT1_LOG" ]; then
        P1_STATUS=$(tail -1 "$PARTICIPANT1_LOG" 2>/dev/null || echo "No output")
        echo "  P1: $P1_STATUS"
    fi

    if [ -f "$PARTICIPANT2_LOG" ]; then
        P2_STATUS=$(tail -1 "$PARTICIPANT2_LOG" 2>/dev/null || echo "No output")
        echo "  P2: $P2_STATUS"
    fi

    echo -e "\n${YELLOW}Check logs for details:${NC}"
    echo "  - $COORDINATOR_LOG"
    echo "  - $PARTICIPANT1_LOG"
    echo "  - $PARTICIPANT2_LOG"

    exit 1
fi