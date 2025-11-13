use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub id: String,
    pub intent_data: String,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub state: String,
    pub txid: Option<String>,
    pub total_fees: Option<i64>,
    pub participant_count: i64,
    pub raw_transaction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchParticipant {
    pub id: i64,
    pub batch_id: String,
    pub pubkey: String,
    pub joined_at: i64,
    pub commitment: Option<String>,
    pub reveal: Option<String>,
    pub signed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub date: String,
    pub batches_completed: i64,
    pub total_participants: i64,
    pub total_fees_saved: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub key: String,
    pub value: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ban {
    pub pubkey: String,
    pub banned_at: i64,
    pub expires_at: Option<i64>,
    pub offense_count: i64,
    pub reason: Option<String>,
}
