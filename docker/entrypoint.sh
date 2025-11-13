#!/bin/bash
set -e

echo "🚀 Bitcoin Batch Coordinator - Starting..."
echo "   Network: $NETWORK"
echo "   Database: $DATABASE_URL"
echo "   API Port: $API_PORT"

# Function to start coordinator daemon
start_coordinator() {
    echo "🎯 Attempting to start Coordinator Daemon..."

    # Start coordinator daemon in background (it will auto-read passphrase from DB)
    /app/coordinator-daemon \
        --database-url "$DATABASE_URL" \
        --network "$NETWORK" \
        --min-participants "$MIN_PARTICIPANTS" \
        --max-participants "$MAX_PARTICIPANTS" \
        --deadline-ms "$((TIMEOUT_SECONDS * 1000))" \
        --multi-batch \
        > /tmp/coordinator.log 2>&1 &

    COORDINATOR_PID=$!
    echo "✅ Coordinator daemon started (PID: $COORDINATOR_PID)"
    echo "   Logs: /tmp/coordinator.log"
}

# Try to start coordinator daemon (will fail gracefully if no identity exists)
# The daemon will auto-read the passphrase from the database
start_coordinator || echo "⚠️  Coordinator daemon not started. Import an identity via the dashboard first."

# Start the API gateway in foreground
echo "🌐 Starting API Gateway..."
exec /app/api-gateway \
  --host "$API_HOST" \
  --port "$API_PORT" \
  --database-url "$DATABASE_URL" \
  --web-dir "$WEB_DIR"
