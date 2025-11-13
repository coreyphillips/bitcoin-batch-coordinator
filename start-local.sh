#!/bin/bash
set -e

echo "🚀 Bitcoin Batch Coordinator - Local Development Setup"
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_DIR"

# Step 1: Check prerequisites
echo -e "${BLUE}Step 1: Checking prerequisites...${NC}"
command -v cargo >/dev/null 2>&1 || { echo "❌ Rust/Cargo not found. Install from https://rustup.rs/"; exit 1; }
command -v node >/dev/null 2>&1 || { echo "❌ Node.js not found. Install from https://nodejs.org/"; exit 1; }
echo -e "${GREEN}✅ Prerequisites OK${NC}"
echo ""

# Step 2: Build frontend
echo -e "${BLUE}Step 2: Building frontend...${NC}"
cd web-ui
if [ ! -d "node_modules" ]; then
    echo "Installing npm dependencies..."
    npm install
fi
npm run build
cd ..
echo -e "${GREEN}✅ Frontend built${NC}"
echo ""

# Step 3: Build backend
echo -e "${BLUE}Step 3: Building backend (this may take a few minutes)...${NC}"
if [ ! -f "target/release/api-gateway" ] || [ ! -f "target/release/coordinator-daemon" ]; then
    cargo build --release
else
    echo "Binaries already built. Run 'cargo build --release' to rebuild."
fi
echo -e "${GREEN}✅ Backend built${NC}"
echo ""

# Step 4: Create data directory
mkdir -p data
echo -e "${GREEN}✅ Data directory ready${NC}"
echo ""

# Step 5: Instructions
echo -e "${YELLOW}========================================${NC}"
echo -e "${GREEN}✅ Build Complete!${NC}"
echo -e "${YELLOW}========================================${NC}"
echo ""
echo "Quick Start:"
echo ""
echo -e "${BLUE}Just run this script - it will start both services!${NC}"
echo ""
echo "  ./start-local.sh"
echo ""
echo -e "${YELLOW}What happens:${NC}"
echo "1. Coordinator daemon starts in background (auto-reads passphrase)"
echo "2. API Gateway + Dashboard start in foreground"
echo "3. Open http://localhost:3000 and import your identity"
echo "4. Both services work together automatically!"
echo "5. Press Ctrl+C to stop both"
echo ""
echo -e "${GREEN}See TESTING.md for detailed instructions!${NC}"
echo ""

# Offer to start both services
read -p "Start both API Gateway + Coordinator now? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo ""
    echo -e "${GREEN}🎯 Starting Coordinator Daemon in background...${NC}"

    # Start coordinator daemon in background
    ./target/release/coordinator-daemon \
        --database-url "sqlite:///$PROJECT_DIR/data/coordinator.db" \
        --network regtest \
        > "$PROJECT_DIR/data/coordinator.log" 2>&1 &

    COORDINATOR_PID=$!
    echo -e "${GREEN}✅ Coordinator started (PID: $COORDINATOR_PID)${NC}"
    echo -e "${YELLOW}   Logs: $PROJECT_DIR/data/coordinator.log${NC}"
    echo -e "${YELLOW}   Note: Coordinator will auto-read passphrase after you import identity${NC}"
    echo ""

    # Give it a moment to start
    sleep 1

    echo -e "${GREEN}🌐 Starting API Gateway...${NC}"
    echo -e "${YELLOW}Dashboard will be available at: http://localhost:3000${NC}"
    echo ""
    echo -e "${YELLOW}Press Ctrl+C to stop both services${NC}"
    echo ""

    # Trap to kill coordinator on exit
    trap "echo ''; echo 'Stopping coordinator...'; kill $COORDINATOR_PID 2>/dev/null; exit" INT TERM EXIT

    # Start API gateway in foreground
    ./target/release/api-gateway \
        --host 0.0.0.0 \
        --port 3000 \
        --database-url "sqlite:///$PROJECT_DIR/data/coordinator.db" \
        --web-dir "$PROJECT_DIR/web-ui/dist"
fi
