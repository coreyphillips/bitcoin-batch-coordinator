#!/bin/bash
set -e

echo "🚀 Bitcoin Batch Coordinator - Starting..."
echo "   Network: $NETWORK"
echo "   Database: $DATABASE_URL"
echo "   API Port: $API_PORT"

# Function to start coordinator daemon
start_coordinator() {
    echo "🎯 Starting Coordinator Daemon..."

    # Check if passphrase is provided
    if [ -z "$COORDINATOR_PASSPHRASE" ]; then
        echo "⚠️  COORDINATOR_PASSPHRASE not set. Coordinator daemon will not start automatically."
        echo "   You can start it manually after importing an identity via the web dashboard."
        return
    fi

    # Start coordinator daemon in background
    /app/coordinator-daemon \
        --database-url "$DATABASE_URL" \
        --passphrase "$COORDINATOR_PASSPHRASE" \
        --network "$NETWORK" \
        --min-participants "$MIN_PARTICIPANTS" \
        --max-participants "$MAX_PARTICIPANTS" \
        --deadline-ms "$((TIMEOUT_SECONDS * 1000))" \
        --multi-batch \
        &

    COORDINATOR_PID=$!
    echo "✅ Coordinator daemon started (PID: $COORDINATOR_PID)"
}

# Try to start coordinator daemon (will fail gracefully if no identity exists)
start_coordinator || echo "⚠️  Coordinator daemon not started. Import an identity via the dashboard first."

# Start the API gateway in foreground
echo "🌐 Starting API Gateway..."
exec /app/api-gateway \
  --host "$API_HOST" \
  --port "$API_PORT" \
  --database-url "$DATABASE_URL" \
  --web-dir "$WEB_DIR"
