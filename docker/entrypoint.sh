#!/bin/bash
set -e

echo "🚀 Bitcoin Batch Coordinator - Starting..."
echo "   Network: $NETWORK"
echo "   Database: $DATABASE_URL"
echo "   API Port: $API_PORT"

# Start the API gateway (which includes the coordinator)
exec /app/api-gateway \
  --host "$API_HOST" \
  --port "$API_PORT" \
  --database-url "$DATABASE_URL" \
  --web-dir "$WEB_DIR"
