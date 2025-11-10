use thiserror::Error;

#[derive(Error, Debug)]
pub enum BatchError {
    #[error("Bitcoin error: {0}")]
    Bitcoin(#[from] bitcoin::consensus::encode::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Invalid commitment: expected {expected}, got {actual}")]
    InvalidCommitment { expected: String, actual: String },

    #[error("Invalid reveal: {0}")]
    InvalidReveal(String),

    #[error("Timeout waiting for {0}")]
    Timeout(String),

    #[error("Insufficient funds: need {need}, have {have}")]
    InsufficientFunds { need: u64, have: u64 },

    #[error("Dust output: {amount} sats is below 546")]
    DustOutput { amount: u64 },

    #[error("Duplicate input: {0}")]
    DuplicateInput(String),

    #[error("Invalid PSBT: {0}")]
    InvalidPsbt(String),

    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Network mismatch: expected {expected}, got {actual}")]
    NetworkMismatch { expected: String, actual: String },

    #[error("Participant dropped: {0}")]
    ParticipantDropped(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BatchError>;
