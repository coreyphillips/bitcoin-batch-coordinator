# Local Testing Guide

## Prerequisites

1. **Rust** (1.75+)
2. **Node.js** (20+)
3. **A Pubky Identity** - You'll need either:
   - A `.pkarr` file with passphrase, OR
   - A 12 or 24-word recovery phrase

## Quick Start (5 minutes)

### Step 1: Build Everything

```bash
# From project root
cd /home/user/bitcoin-batch-coordinator

# Build the frontend
cd web-ui
npm install
npm run build
cd ..

# Build the backend (this will take a few minutes)
cargo build --release
```

### Step 2: Start the API Gateway + Dashboard

```bash
# Create data directory
mkdir -p data

# Start the API gateway (includes web UI)
./target/release/api-gateway \
  --host 0.0.0.0 \
  --port 3000 \
  --database-url sqlite:///$(pwd)/data/coordinator.db \
  --web-dir $(pwd)/web-ui/dist
```

You should see:
```
🚀 Starting Bitcoin Batch Coordinator API Gateway
   Version: 0.1.0
   API Server: 0.0.0.0:3000
   Database: sqlite:///data/coordinator.db
📦 Connecting to database...
✅ Database connected and migrations applied
🌐 Listening on 0.0.0.0:3000
```

### Step 3: Access the Dashboard

Open your browser: **http://localhost:3000**

You'll see the **Identity Setup Screen** (you can't access the dashboard until you import an identity):

```
┌─────────────────────────────────────┐
│  Setup Coordinator Identity         │
│                                     │
│  Import your identity to continue   │
│                                     │
│  [Import File] [Import Phrase]      │
└─────────────────────────────────────┘
```

### Step 4: Import Your Identity

**Option A: Import .pkarr File**
1. Click "Import File" tab
2. Drag & drop your `.pkarr` file
3. Enter your passphrase
4. Click "Import Identity"

**Option B: Import Recovery Phrase**
1. Click "Import Phrase" tab
2. Enter your 12 or 24-word recovery phrase
3. Enter a passphrase to encrypt it
4. Click "Import from Phrase"

After successful import, you'll see:
```
✅ Identity imported! Pubkey: xyz123abc...
```

The page will reload and show the **Dashboard**.

### Step 5: Start the Coordinator Daemon

Open a **new terminal** (keep the API gateway running):

```bash
cd /home/user/bitcoin-batch-coordinator

# Run the coordinator daemon
./target/release/coordinator-daemon \
  --database-url sqlite:///$(pwd)/data/coordinator.db \
  --network signet \
  --min-participants 2 \
  --max-participants 10 \
  --deadline-ms 300000
```

It will prompt for your passphrase:
```
🚀 Starting Bitcoin Batch Coordinator Daemon
   Version: 0.1.0
   Database: sqlite:///data/coordinator.db
   Network: signet
📦 Connecting to database...
✅ Database connected
🔑 Fetching coordinator identity...
✅ Found phrase identity: xyz123abc...
🔐 Enter passphrase to decrypt identity:
```

Enter the same passphrase you used when importing, then you'll see:
```
🔓 Decrypting identity...
✅ Identity decrypted successfully
📝 Using recovery phrase
🎯 Starting coordinator with identity: xyz123abc...
   Network: signet
   Fee rate: 10 sat/vB
   Participants: 2-10
   Deadline: 300000ms
   Multi-batch: true

Starting coordinator
Network: signet, Fee rate: 10 sat/vB
Participants: 2-10, Deadline: 300000ms
Initializing pubky-messenger...
Coordinator pubky: xyz123abc...
Clearing old messages from previous coordinator sessions...
Discovering peers from Pubky follow graph...
Created initial batch with intent_id: ...
```

🎉 **Your coordinator is now running!**

## What You Should See

### Terminal 1 (API Gateway)
```
GET /api/v1/coordinator/status 200
GET /api/v1/batches 200
GET /api/v1/stats 200
```

### Terminal 2 (Coordinator)
```
Received GetCurrentIntent from participant_xyz...
Commitment phase complete with X participants
Phase 2: Requesting reveals...
```

### Browser (Dashboard)
- **Stats Cards**: Total saved, active batches, participants
- **Active Batches**: List of current batches being coordinated
- **Recent Activity**: Transaction history

## Testing with a Test Identity

If you don't have a real identity, you can generate one for testing:

```bash
# Generate a test recovery phrase (12 words)
# You can use any BIP39 mnemonic generator, or this quick one:
cargo run --bin coordinator -- --help

# Or use this test phrase (DO NOT USE FOR REAL BITCOIN):
abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about
```

Then in the dashboard:
1. Click "Import Phrase"
2. Paste: `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
3. Enter any passphrase (remember it!)
4. Import

⚠️ **WARNING**: This is a well-known test mnemonic. Never use it with real funds!

## Troubleshooting

### "No identity found in database"
- You need to import an identity via the web dashboard first
- Go to http://localhost:3000 and complete the identity setup

### "Failed to decrypt identity"
- Wrong passphrase entered
- Make sure you're using the same passphrase you used when importing

### "Address already in use"
- Port 3000 is already taken
- Change the port: `--port 3001`

### Database locked
- Only one process can write to SQLite at a time
- This is normal - the coordinator and API share the database

## Advanced: Running with Docker

```bash
# Build the Docker image
docker build -t bitcoin-batch-coordinator .

# Run it
docker run -d \
  -p 3000:3000 \
  -v $(pwd)/data:/data \
  -e COORDINATOR_PASSPHRASE="your-passphrase-here" \
  -e NETWORK=signet \
  bitcoin-batch-coordinator
```

With Docker, both services run in one container automatically!

## Next Steps

1. **Test with a participant**: Use the participant CLI to join a batch
2. **Monitor batches**: Watch the coordinator create and fill batches
3. **Check the database**: `sqlite3 data/coordinator.db "SELECT * FROM batches;"`
4. **View logs**: Both terminals show detailed logging

## Common Commands

```bash
# Check coordinator status via API
curl http://localhost:3000/api/v1/coordinator/status

# Check batches
curl http://localhost:3000/api/v1/batches

# Check your identity
curl http://localhost:3000/api/v1/identity/current

# View database
sqlite3 data/coordinator.db ".tables"
sqlite3 data/coordinator.db "SELECT * FROM coordinator_identity;"
```

---

Need help? The coordinator logs will tell you exactly what's happening at each step!
