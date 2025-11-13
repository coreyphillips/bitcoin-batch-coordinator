#!/bin/bash
set -e

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_DIR"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}🎯 Starting Bitcoin Batch Coordinator Daemon${NC}"
echo ""

# Check if binary exists
if [ ! -f "target/release/coordinator-daemon" ]; then
    echo -e "${RED}❌ Coordinator daemon not built!${NC}"
    echo "Run: cargo build --release"
    exit 1
fi

# Check if database exists
if [ ! -f "data/coordinator.db" ]; then
    echo -e "${YELLOW}⚠️  Database not found. Starting API gateway first will create it.${NC}"
    echo "The database will be created when you start the API gateway."
    read -p "Continue anyway? (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Parse arguments or use defaults
NETWORK="${NETWORK:-regtest}"
MIN_PARTICIPANTS="${MIN_PARTICIPANTS:-2}"
MAX_PARTICIPANTS="${MAX_PARTICIPANTS:-10}"
DEADLINE_MS="${DEADLINE_MS:-300000}"

echo "Configuration:"
echo "  Network: $NETWORK"
echo "  Min Participants: $MIN_PARTICIPANTS"
echo "  Max Participants: $MAX_PARTICIPANTS"
echo "  Deadline: ${DEADLINE_MS}ms"
echo ""

# Start coordinator daemon
./target/release/coordinator-daemon \
    --database-url "sqlite:///$PROJECT_DIR/data/coordinator.db" \
    --network "$NETWORK" \
    --min-participants "$MIN_PARTICIPANTS" \
    --max-participants "$MAX_PARTICIPANTS" \
    --deadline-ms "$DEADLINE_MS" \
    --multi-batch \
    "$@"
