# Bitcoin Batch Coordinator - Umbrel App Architecture

## Overview

This document outlines the architecture for transforming the Bitcoin Batch Coordinator into a beautiful Umbrel app with a modern web dashboard.

## System Components

```
┌─────────────────────────────────────────────────────────────────┐
│                         Umbrel Host                              │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                   Bitcoin Batch Coordinator App             │ │
│  │                                                              │ │
│  │  ┌──────────────┐      ┌─────────────────┐                 │ │
│  │  │   Web UI     │◄────►│   API Gateway   │                 │ │
│  │  │  (React/TS)  │      │  (Rust + Axum)  │                 │ │
│  │  │              │      │                 │                 │ │
│  │  │  - Dashboard │      │  - REST API     │                 │ │
│  │  │  - Batch Mgmt│      │  - WebSocket    │                 │ │
│  │  │  - History   │      │  - SSE Events   │                 │ │
│  │  │  - Settings  │      └────────┬────────┘                 │ │
│  │  └──────────────┘               │                          │ │
│  │         │                       │                          │ │
│  │         │                       ▼                          │ │
│  │         │              ┌─────────────────┐                 │ │
│  │         │              │  Coordinator    │                 │ │
│  │         │              │  Core (Rust)    │                 │ │
│  │         │              │                 │                 │ │
│  │         │              │  - CoordV2      │                 │ │
│  │         │              │  - Batch Mgr    │                 │ │
│  │         │              │  - Ban Mgr      │                 │ │
│  │         │              └────────┬────────┘                 │ │
│  │         │                       │                          │ │
│  │         │                       ▼                          │ │
│  │         │              ┌─────────────────┐                 │ │
│  │         │              │  Data Layer     │                 │ │
│  │         │              │                 │                 │ │
│  │         │              │  - SQLite DB    │                 │ │
│  │         │              │  - Ban List     │                 │ │
│  │         │              │  - Config       │                 │ │
│  │         └─────────────►│  - Batch Hist.  │                 │ │
│  │                        └─────────────────┘                 │ │
│  │                                 │                          │ │
│  └─────────────────────────────────┼──────────────────────────┘ │
│                                    │                            │
│  ┌─────────────────────────────────▼──────────────────────────┐ │
│  │              Umbrel Bitcoin/Electrum Services              │ │
│  │  - Bitcoin Core (RPC)                                      │ │
│  │  - Electrum Server                                         │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                    │                            │
└────────────────────────────────────┼────────────────────────────┘
                                     │
                          ┌──────────▼──────────┐
                          │   Pubky Network     │
                          │  (P2P Messaging)    │
                          └─────────────────────┘
```

## Service Breakdown

### 1. Web Dashboard (Frontend)
**Technology**: React + TypeScript + Vite
**Purpose**: Beautiful, responsive UI for batch management

**Features**:
- **Home Dashboard**
  - Live batch status (filling, ready, signing)
  - Real-time participant counter
  - Fee savings calculator
  - Recent batches timeline

- **Batch Management**
  - Create new batch intent
  - View active batches
  - Join existing batches
  - Batch details modal (participants, fees, transaction)

- **History**
  - Completed batches table
  - Transaction links to explorers
  - Fee savings over time chart
  - Export to CSV

- **Settings**
  - Network selection (mainnet/testnet/signet)
  - Electrum server configuration
  - Batch parameters (min/max participants, timeout)
  - Pubky identity management
  - Ban list viewer

**UI Framework**: Shadcn/ui + Tailwind CSS
**Charts**: Recharts
**State Management**: Zustand or TanStack Query
**Real-time**: WebSocket client

### 2. API Gateway
**Technology**: Rust + Axum web framework
**Purpose**: HTTP/WebSocket bridge to coordinator

**Endpoints**:

```rust
// REST API
GET  /api/v1/status                  // Coordinator health
GET  /api/v1/batches                 // List all batches
GET  /api/v1/batches/:id             // Batch details
POST /api/v1/batches                 // Create new batch intent
GET  /api/v1/batches/:id/participants // List participants
GET  /api/v1/history                 // Completed batches
GET  /api/v1/stats                   // Fee savings, totals
GET  /api/v1/config                  // Current configuration
PUT  /api/v1/config                  // Update configuration
GET  /api/v1/bans                    // Ban list
DELETE /api/v1/bans/:pubkey          // Unban participant

// WebSocket
WS   /api/v1/ws                      // Real-time events

// SSE (Server-Sent Events - alternative to WS)
GET  /api/v1/events                  // SSE stream
```

**WebSocket Events**:
```typescript
// Client → Server
{ type: "subscribe", channels: ["batches", "participants"] }
{ type: "unsubscribe", channels: ["batches"] }

// Server → Client
{ type: "batch.created", data: { id, params, timestamp } }
{ type: "batch.updated", data: { id, state, participants } }
{ type: "batch.completed", data: { id, txid, fees } }
{ type: "participant.joined", data: { batch_id, pubkey } }
{ type: "participant.banned", data: { pubkey, reason } }
```

**Dependencies**:
```toml
axum = "0.7"              # Web framework
tower = "0.4"             # Middleware
tower-http = "0.5"        # CORS, logging
tokio = { version = "1", features = ["full"] }
serde = "1.0"
serde_json = "1.0"
sqlx = "0.7"              # SQLite async driver
```

### 3. Coordinator Core (Enhanced)
**Technology**: Existing Rust coordinator with modifications
**Purpose**: Core batch coordination logic

**Modifications Needed**:
1. **Add event bus** for API layer to subscribe to
2. **Add persistence layer** for batch history
3. **Expose coordinator state** via channels/Arc<RwLock>
4. **Add configuration reload** without restart

**New Module**: `coordinator/src/events.rs`
```rust
pub enum CoordinatorEvent {
    BatchCreated { id: String, intent: Intent },
    BatchStateChanged { id: String, state: BatchState },
    ParticipantJoined { batch_id: String, pubkey: String },
    BatchCompleted { id: String, txid: String, total_fees: u64 },
    ParticipantBanned { pubkey: String, reason: String },
}

pub type EventSender = tokio::sync::broadcast::Sender<CoordinatorEvent>;
```

### 4. Data Layer
**Technology**: SQLite + sqlx
**Purpose**: Persistent storage for history and configuration

**Schema**:
```sql
-- Batch history
CREATE TABLE batches (
    id TEXT PRIMARY KEY,
    intent_data TEXT NOT NULL,      -- JSON serialized Intent
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    state TEXT NOT NULL,            -- "filling", "ready", "signing", "completed", "failed"
    txid TEXT,
    total_fees INTEGER,
    participant_count INTEGER,
    raw_transaction TEXT
);

-- Participants per batch
CREATE TABLE batch_participants (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT NOT NULL,
    pubkey TEXT NOT NULL,
    joined_at INTEGER NOT NULL,
    commitment TEXT,
    reveal TEXT,
    signed BOOLEAN DEFAULT FALSE,
    FOREIGN KEY (batch_id) REFERENCES batches(id)
);

-- Fee savings stats
CREATE TABLE stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,             -- YYYY-MM-DD
    batches_completed INTEGER DEFAULT 0,
    total_participants INTEGER DEFAULT 0,
    total_fees_saved INTEGER DEFAULT 0,
    UNIQUE(date)
);

-- Configuration
CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Ban list (complement to existing ban_list.json)
CREATE TABLE bans (
    pubkey TEXT PRIMARY KEY,
    banned_at INTEGER NOT NULL,
    expires_at INTEGER,
    offense_count INTEGER NOT NULL,
    reason TEXT
);
```

## Docker Services

### docker-compose.yml Structure
```yaml
version: "3.8"

services:
  coordinator:
    build: .
    container_name: bitcoin-batch-coordinator
    environment:
      - NETWORK=${NETWORK:-bitcoin}
      - MIN_PARTICIPANTS=${MIN_PARTICIPANTS:-2}
      - MAX_PARTICIPANTS=${MAX_PARTICIPANTS:-10}
      - TIMEOUT_SECONDS=${TIMEOUT_SECONDS:-300}
      - ELECTRUM_HOST=${APP_BITCOIN_ELECTRUM_HOST}
      - ELECTRUM_PORT=${APP_BITCOIN_ELECTRUM_PORT}
      - API_PORT=3000
      - WEB_PORT=3001
    volumes:
      - ${APP_DATA_DIR}/data:/data
      - ${APP_DATA_DIR}/config:/config
    ports:
      - "${APP_BITCOIN_BATCH_COORDINATOR_PORT}:3001"
    restart: unless-stopped
```

### Dockerfile (Multi-stage)
```dockerfile
# Stage 1: Build Rust binaries
FROM rust:1.91-slim as builder
WORKDIR /build
COPY . .
RUN cargo build --release

# Stage 2: Build Web UI
FROM node:20-alpine as ui-builder
WORKDIR /app
COPY web-ui/package*.json ./
RUN npm ci
COPY web-ui/ ./
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binaries
COPY --from=builder /build/target/release/coordinator /app/
COPY --from=builder /build/target/release/api-gateway /app/

# Copy web UI
COPY --from=ui-builder /app/dist /app/web

# Copy config
COPY electrum-servers.toml /app/

# Create data directories
RUN mkdir -p /data /config

EXPOSE 3000 3001

# Startup script
COPY docker/entrypoint.sh /app/
RUN chmod +x /app/entrypoint.sh

ENTRYPOINT ["/app/entrypoint.sh"]
```

## Development Phases

### Phase 1: Foundation (Week 1)
- [x] Architecture design
- [ ] Create API gateway crate structure
- [ ] Implement basic REST endpoints
- [ ] Add SQLite database with schema
- [ ] Modify coordinator to emit events
- [ ] Docker containerization

### Phase 2: API Layer (Week 2)
- [ ] Complete all REST endpoints
- [ ] WebSocket implementation
- [ ] Event broadcasting system
- [ ] API documentation (OpenAPI/Swagger)
- [ ] Integration tests

### Phase 3: Web Dashboard (Week 2-3)
- [ ] Project setup (Vite + React + TypeScript)
- [ ] UI component library (Shadcn/ui)
- [ ] Dashboard page
- [ ] Batch management page
- [ ] History page with charts
- [ ] Settings page
- [ ] Real-time WebSocket integration

### Phase 4: Umbrel Integration (Week 3)
- [ ] Create umbrel-app.yml manifest
- [ ] Create exports.sh
- [ ] Icon and gallery images
- [ ] Integration with Umbrel Bitcoin/Electrum
- [ ] Testing on Umbrel testnet

### Phase 5: Polish (Week 4)
- [ ] Error handling and validation
- [ ] Loading states and animations
- [ ] Responsive design refinement
- [ ] Documentation
- [ ] Submit to Umbrel App Store

## File Structure

```
bitcoin-batch-coordinator/
├── coordinator/              # Existing coordinator
├── participant/              # Existing participant
├── common/                   # Existing common
├── api-gateway/              # NEW: API service
│   ├── src/
│   │   ├── main.rs
│   │   ├── routes/
│   │   │   ├── batches.rs
│   │   │   ├── history.rs
│   │   │   ├── stats.rs
│   │   │   └── websocket.rs
│   │   ├── db/
│   │   │   ├── mod.rs
│   │   │   ├── schema.rs
│   │   │   └── queries.rs
│   │   └── state.rs
│   └── Cargo.toml
├── web-ui/                   # NEW: Frontend
│   ├── src/
│   │   ├── App.tsx
│   │   ├── pages/
│   │   │   ├── Dashboard.tsx
│   │   │   ├── Batches.tsx
│   │   │   ├── History.tsx
│   │   │   └── Settings.tsx
│   │   ├── components/
│   │   │   ├── BatchCard.tsx
│   │   │   ├── ParticipantList.tsx
│   │   │   ├── FeeChart.tsx
│   │   │   └── ui/          # Shadcn components
│   │   ├── hooks/
│   │   │   ├── useWebSocket.ts
│   │   │   └── useBatches.ts
│   │   └── lib/
│   │       ├── api.ts
│   │       └── utils.ts
│   ├── package.json
│   ├── vite.config.ts
│   └── tailwind.config.js
├── docker/                   # NEW: Docker configs
│   ├── Dockerfile
│   ├── entrypoint.sh
│   └── nginx.conf           # For serving web UI
├── umbrel/                   # NEW: Umbrel integration
│   ├── umbrel-app.yml
│   ├── exports.sh
│   ├── docker-compose.yml
│   └── icon.svg
└── UMBREL_ARCHITECTURE.md   # This file
```

## API Gateway Implementation Details

### State Management
```rust
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub event_bus: EventSender,
    pub coordinator_handle: Arc<RwLock<CoordinatorHandle>>,
}

pub struct CoordinatorHandle {
    pub current_batches: HashMap<String, BatchInfo>,
    pub stats: Stats,
}
```

### Middleware Stack
1. **CORS**: Allow web UI access
2. **Logging**: Request/response logging
3. **Compression**: gzip responses
4. **Rate limiting**: Prevent abuse
5. **Auth (future)**: API key or Umbrel SSO

## UI Design Principles

### Visual Style
- **Color Scheme**: Bitcoin orange (#F7931A) + dark mode
- **Typography**: Inter for UI, JetBrains Mono for addresses/txids
- **Components**: Card-based layout, glass morphism effects
- **Animations**: Smooth transitions, loading skeletons

### Key Screens

**Dashboard**:
- Hero section with total savings
- Active batches grid (live updating)
- Quick action buttons
- Recent activity feed

**Batch Detail Modal**:
- Progress stepper (Filling → Ready → Signing → Complete)
- Participant avatars (generated from pubkey)
- Transaction preview
- Copy buttons for txid, batch ID

**History**:
- Filterable table (date range, status)
- Sortable columns
- Fee savings chart (daily/weekly/monthly)
- Export functionality

**Settings**:
- Network selector with testnet warning
- Electrum server status indicator
- Batch parameter sliders
- Identity QR code for mobile

## Security Considerations

1. **API Rate Limiting**: Prevent DoS
2. **Input Validation**: All endpoints validate params
3. **CORS**: Restrict to Umbrel domain only
4. **WebSocket Auth**: Token-based or Umbrel session
5. **Database**: Prepared statements prevent SQL injection
6. **File Permissions**: Restrict /data and /config directories

## Performance Targets

- **API Response Time**: < 100ms (p95)
- **WebSocket Latency**: < 50ms
- **Page Load**: < 2s (First Contentful Paint)
- **Database Queries**: < 10ms for most queries
- **Memory Usage**: < 256MB total
- **CPU Usage**: < 5% idle, < 20% during batch

## Future Enhancements

- [ ] Multi-coordinator support (connect to remote coordinators)
- [ ] Participant mode in UI (join batches from other coordinators)
- [ ] Notifications (push, email when batch completes)
- [ ] Advanced analytics (fee comparison to solo transactions)
- [ ] API webhooks for developers
- [ ] Mobile-responsive PWA
- [ ] Lightning integration for coordinator fees
- [ ] Multi-language support

## Testing Strategy

### Unit Tests
- API endpoint logic
- Database queries
- Event bus
- Coordinator modifications

### Integration Tests
- Full batch flow via API
- WebSocket event delivery
- Database persistence

### E2E Tests (Playwright)
- Dashboard user flows
- Batch creation and monitoring
- Settings management

### Docker Testing
- Build and run locally
- Test on Umbrel testnet
- Performance benchmarks

## Deployment

### Local Development
```bash
# Terminal 1: Run coordinator with API
cargo run --bin api-gateway

# Terminal 2: Run web UI dev server
cd web-ui && npm run dev
```

### Umbrel Deployment
```bash
# Build and tag Docker image
docker build -t bitcoin-batch-coordinator:v1.0.0 .

# Push to Docker Hub
docker push your-dockerhub/bitcoin-batch-coordinator:v1.0.0

# Submit to Umbrel App Store
# Create PR to getumbrel/umbrel-apps
```

## Success Metrics

- ✅ App installs on Umbrel without errors
- ✅ Successfully coordinates batches with 2+ participants
- ✅ Web UI is responsive and beautiful
- ✅ Real-time updates work reliably
- ✅ Fee savings are accurately calculated and displayed
- ✅ Users can configure and manage batches easily
- ✅ Integration with Umbrel's Bitcoin/Electrum is seamless

---

**Next Steps**: Begin Phase 1 by creating the API gateway structure and database schema.
