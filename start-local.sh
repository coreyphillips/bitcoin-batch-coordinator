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
echo "To start the system:"
echo ""
echo -e "${BLUE}Terminal 1: Start API Gateway + Dashboard${NC}"
echo "  ./target/release/api-gateway \\"
echo "    --host 0.0.0.0 \\"
echo "    --port 3000 \\"
echo "    --database-url sqlite:///$PROJECT_DIR/data/coordinator.db \\"
echo "    --web-dir $PROJECT_DIR/web-ui/dist"
echo ""
echo -e "${BLUE}Terminal 2: Start Coordinator Daemon (after importing identity)${NC}"
echo "  ./target/release/coordinator-daemon \\"
echo "    --database-url sqlite:///$PROJECT_DIR/data/coordinator.db \\"
echo "    --network signet"
echo ""
echo -e "${YELLOW}Next steps:${NC}"
echo "1. Start the API Gateway (Terminal 1 command above)"
echo "2. Open http://localhost:3000 in your browser"
echo "3. Import your identity (file or recovery phrase)"
echo "4. Start the Coordinator Daemon (Terminal 2 command above)"
echo ""
echo -e "${GREEN}See TESTING.md for detailed instructions!${NC}"
echo ""

# Offer to start API gateway
read -p "Start API Gateway now? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo ""
    echo -e "${GREEN}Starting API Gateway...${NC}"
    echo -e "${YELLOW}Dashboard will be available at: http://localhost:3000${NC}"
    echo ""
    ./target/release/api-gateway \
        --host 0.0.0.0 \
        --port 3000 \
        --database-url "sqlite:///$PROJECT_DIR/data/coordinator.db" \
        --web-dir "$PROJECT_DIR/web-ui/dist"
fi
