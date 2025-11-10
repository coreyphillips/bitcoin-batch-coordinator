use common::{messages::*, BatchError, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

/// Represents a single batch with its state and participants
#[derive(Debug, Clone)]
pub struct Batch {
    pub intent: Intent,
    pub status: BatchStatus,
    pub commitments: HashMap<String, Commitment>,
    pub reveals: HashMap<String, Reveal>,
    pub created_at: std::time::Instant,
}

impl Batch {
    pub fn new(intent: Intent) -> Self {
        Self {
            intent,
            status: BatchStatus::Filling,
            commitments: HashMap::new(),
            reveals: HashMap::new(),
            created_at: std::time::Instant::now(),
        }
    }

    pub fn participant_count(&self) -> usize {
        self.commitments.len()
    }

    pub fn is_full(&self) -> bool {
        self.participant_count() >= self.intent.max_participants
    }

    pub fn has_minimum(&self) -> bool {
        self.participant_count() >= self.intent.min_participants
    }

    pub fn update_status(&mut self) {
        if self.is_full() {
            self.status = BatchStatus::Full;
        } else if self.has_minimum() {
            self.status = BatchStatus::Ready;
        } else {
            self.status = BatchStatus::Filling;
        }
    }
}

/// Manages multiple concurrent batches
pub struct BatchManager {
    /// All active batches by intent_id
    batches: Arc<RwLock<HashMap<Uuid, Batch>>>,
    /// Current active batch (accepting new participants)
    current_intent_id: Arc<RwLock<Option<Uuid>>>,
    /// Network configuration for creating new batches
    network: NetworkSpec,
    fee_rate_sat_vb: u64,
    min_participants: usize,
    max_participants: usize,
    deadline_ms: u64,
    allow_change: bool,
    coordinator_pkarr: String,
}

impl BatchManager {
    pub fn new(
        network: NetworkSpec,
        fee_rate_sat_vb: u64,
        min_participants: usize,
        max_participants: usize,
        deadline_ms: u64,
        allow_change: bool,
        coordinator_pkarr: String,
    ) -> Self {
        Self {
            batches: Arc::new(RwLock::new(HashMap::new())),
            current_intent_id: Arc::new(RwLock::new(None)),
            network,
            fee_rate_sat_vb,
            min_participants,
            max_participants,
            deadline_ms,
            allow_change,
            coordinator_pkarr,
        }
    }

    /// Create a new batch and set it as current
    pub async fn create_new_batch(&self) -> Result<Intent> {
        let intent_id = Uuid::new_v4();
        let intent = Intent {
            intent_id,
            network: self.network.clone(),
            fee_rate_sat_vb: self.fee_rate_sat_vb,
            min_participants: self.min_participants,
            max_participants: self.max_participants,
            deadline_ms: self.deadline_ms,
            allow_change: self.allow_change,
            coordinator_pkarr: self.coordinator_pkarr.clone(),
            fee_model: FeeModel::WeightBased, // Using fair weight-based fee calculation
        };

        let batch = Batch::new(intent.clone());

        let mut batches = self.batches.write().await;
        batches.insert(intent_id, batch);

        let mut current = self.current_intent_id.write().await;
        *current = Some(intent_id);

        info!("Created new batch with intent_id: {}", intent_id);

        Ok(intent)
    }

    /// Get the current active batch intent
    pub async fn get_current_intent(&self) -> Result<Option<CurrentIntent>> {
        let current_id = self.current_intent_id.read().await;

        if let Some(intent_id) = *current_id {
            let batches = self.batches.read().await;
            if let Some(batch) = batches.get(&intent_id) {
                return Ok(Some(CurrentIntent {
                    intent: batch.intent.clone(),
                    status: batch.status.clone(),
                    current_participants: batch.participant_count(),
                }));
            }
        }

        Ok(None)
    }

    /// Get or create a current batch
    pub async fn get_or_create_current_intent(&self) -> Result<CurrentIntent> {
        // Try to get existing current batch
        if let Some(current) = self.get_current_intent().await? {
            // Check if current batch is full
            if matches!(current.status, BatchStatus::Full) {
                // Create a new batch since current is full
                info!("Current batch is full, creating new batch");
                let intent = self.create_new_batch().await?;
                return Ok(CurrentIntent {
                    intent,
                    status: BatchStatus::Filling,
                    current_participants: 0,
                });
            }
            return Ok(current);
        }

        // No current batch, create one
        info!("No current batch found, creating new batch");
        let intent = self.create_new_batch().await?;
        Ok(CurrentIntent {
            intent,
            status: BatchStatus::Filling,
            current_participants: 0,
        })
    }

    /// Add a commitment to a batch
    pub async fn add_commitment(&self, intent_id: Uuid, commitment: Commitment) -> Result<()> {
        let mut batches = self.batches.write().await;

        if let Some(batch) = batches.get_mut(&intent_id) {
            if batch.is_full() {
                return Err(BatchError::Other("Batch is full".to_string()));
            }

            batch
                .commitments
                .insert(commitment.participant_pkarr.clone(), commitment);
            batch.update_status();

            info!(
                "Added commitment to batch {}. Total participants: {}, Status: {:?}",
                intent_id,
                batch.participant_count(),
                batch.status
            );

            // If this batch is now full and it's the current batch, clear current
            if batch.is_full() {
                let mut current = self.current_intent_id.write().await;
                if *current == Some(intent_id) {
                    info!(
                        "Current batch {} is now full, clearing current intent",
                        intent_id
                    );
                    *current = None;
                }
            }

            Ok(())
        } else {
            Err(BatchError::Other(format!("Batch {} not found", intent_id)))
        }
    }

    /// Add a reveal to a batch
    pub async fn add_reveal(&self, intent_id: Uuid, reveal: Reveal) -> Result<()> {
        let mut batches = self.batches.write().await;

        if let Some(batch) = batches.get_mut(&intent_id) {
            batch
                .reveals
                .insert(reveal.participant_pkarr.clone(), reveal);
            Ok(())
        } else {
            Err(BatchError::Other(format!("Batch {} not found", intent_id)))
        }
    }

    /// Get a specific batch
    pub async fn get_batch(&self, intent_id: Uuid) -> Option<Batch> {
        let batches = self.batches.read().await;
        batches.get(&intent_id).cloned()
    }

    /// Update batch status
    pub async fn update_batch_status(&self, intent_id: Uuid, status: BatchStatus) -> Result<()> {
        let mut batches = self.batches.write().await;

        if let Some(batch) = batches.get_mut(&intent_id) {
            batch.status = status.clone();
            info!("Updated batch {} status to {:?}", intent_id, status);

            // If batch is completed, remove it from active batches after a delay
            if matches!(status, BatchStatus::Completed) {
                // In production, you might want to keep completed batches for some time
                // For now, we'll keep them in memory
                debug!(
                    "Batch {} completed, keeping in memory for reference",
                    intent_id
                );
            }

            Ok(())
        } else {
            Err(BatchError::Other(format!("Batch {} not found", intent_id)))
        }
    }

    /// Get all active batches
    pub async fn get_active_batches(&self) -> Vec<(Uuid, BatchStatus, usize)> {
        let batches = self.batches.read().await;
        batches
            .iter()
            .filter(|(_, batch)| !matches!(batch.status, BatchStatus::Completed))
            .map(|(id, batch)| (*id, batch.status.clone(), batch.participant_count()))
            .collect()
    }

    /// Clean up old completed batches
    pub async fn cleanup_completed_batches(&self, older_than_secs: u64) {
        let mut batches = self.batches.write().await;
        let now = std::time::Instant::now();

        batches.retain(|id, batch| {
            if matches!(batch.status, BatchStatus::Completed) {
                let age = now.duration_since(batch.created_at).as_secs();
                if age > older_than_secs {
                    info!("Removing old completed batch {} (age: {}s)", id, age);
                    return false;
                }
            }
            true
        });
    }
}
