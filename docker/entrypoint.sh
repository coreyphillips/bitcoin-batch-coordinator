#!/bin/bash
set -e

echo "🚀 Bitcoin Batch Coordinator - Starting..."
echo "   Network: $NETWORK"
echo "   Database: $DATABASE_URL"
echo "   API Port: $API_PORT"

# Note: Coordinator daemon is NOT auto-started here
# It will be started when you create a batch via the dashboard

# Start the API gateway in foreground
echo "🌐 Starting API Gateway..."
exec /app/api-gateway \
  --host "$API_HOST" \
  --port "$API_PORT" \
  --database-url "$DATABASE_URL" \
  --web-dir "$WEB_DIR"
