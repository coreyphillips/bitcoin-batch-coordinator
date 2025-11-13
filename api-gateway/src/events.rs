use serde::{Deserialize, Serialize};

/// Events emitted by the coordinator that the API layer can broadcast to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoordinatorEvent {
    /// A new batch was created
    BatchCreated {
        id: String,
        min_participants: u32,
        max_participants: u32,
        timeout_seconds: u64,
        created_at: i64,
    },

    /// Batch state changed
    BatchStateChanged {
        id: String,
        state: BatchState,
        participant_count: u32,
    },

    /// A participant joined a batch
    ParticipantJoined {
        batch_id: String,
        pubkey: String,
        timestamp: i64,
    },

    /// A batch completed successfully
    BatchCompleted {
        id: String,
        txid: String,
        total_fees: u64,
        participant_count: u32,
        timestamp: i64,
    },

    /// A batch failed
    BatchFailed {
        id: String,
        reason: String,
        timestamp: i64,
    },

    /// A participant was banned
    ParticipantBanned {
        pubkey: String,
        reason: String,
        offense_count: u32,
        expires_at: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchState {
    Filling,
    Ready,
    Signing,
    Completed,
    Failed,
}
