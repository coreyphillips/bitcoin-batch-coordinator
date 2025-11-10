use chrono::{DateTime, Duration, Utc};
use common::BatchError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Manages the ban list for malicious participants
pub struct BanManager {
    bans: Arc<RwLock<HashMap<String, BanEntry>>>,
    warnings: Arc<RwLock<HashMap<String, WarningEntry>>>,
    storage_path: PathBuf,
}

impl BanManager {
    /// Create a new BanManager with the specified storage path
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            bans: Arc::new(RwLock::new(HashMap::new())),
            warnings: Arc::new(RwLock::new(HashMap::new())),
            storage_path,
        }
    }

    /// Load ban list from disk
    pub async fn load(self) -> Result<Self, BatchError> {
        if self.storage_path.exists() {
            match fs::read_to_string(&self.storage_path).await {
                Ok(contents) => match serde_json::from_str::<BanListStorage>(&contents) {
                    Ok(storage) => {
                        let mut bans = self.bans.write().await;
                        let mut warnings = self.warnings.write().await;

                        for ban in storage.bans {
                            bans.insert(ban.participant_pkarr.clone(), ban);
                        }

                        for warning in storage.warnings {
                            warnings.insert(warning.participant_pkarr.clone(), warning);
                        }

                        info!(
                            "Loaded {} bans and {} warnings from disk",
                            bans.len(),
                            warnings.len()
                        );
                    }
                    Err(e) => {
                        error!("Failed to parse ban list: {}", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read ban list file: {}", e);
                }
            }
        } else {
            info!("No existing ban list found at {:?}", self.storage_path);
        }

        Ok(self)
    }

    /// Save ban list to disk
    pub async fn save(&self) -> Result<(), BatchError> {
        let bans = self.bans.read().await;
        let warnings = self.warnings.read().await;

        let storage = BanListStorage {
            version: 1,
            last_updated: Utc::now(),
            bans: bans.values().cloned().collect(),
            warnings: warnings.values().cloned().collect(),
        };

        let json =
            serde_json::to_string_pretty(&storage).map_err(|e| BatchError::Serialization(e))?;

        fs::write(&self.storage_path, json)
            .await
            .map_err(|e| BatchError::FileSystem(format!("Failed to save ban list: {}", e)))?;

        debug!(
            "Saved {} bans and {} warnings to disk",
            bans.len(),
            warnings.len()
        );
        Ok(())
    }

    /// Check if a participant is currently banned
    pub async fn is_banned(&self, pkarr: &str) -> bool {
        let bans = self.bans.read().await;

        if let Some(ban) = bans.get(pkarr) {
            match &ban.ban_type {
                BanType::Permanent => true,
                BanType::Temporary => {
                    // For temporary bans, check if they've expired
                    if let Some(expires) = ban.expires_at {
                        expires > Utc::now()
                    } else {
                        // Temporary ban without expiry date is treated as permanent
                        true
                    }
                }
            }
        } else {
            false
        }
    }

    /// Record an offense and determine appropriate action
    pub async fn record_offense(
        &self,
        pkarr: &str,
        offense: Offense,
    ) -> Result<BanAction, BatchError> {
        let action = match offense.severity() {
            OffenseSeverity::Critical => {
                // Critical offenses get permanent ban immediately
                self.add_permanent_ban(
                    pkarr,
                    offense.to_string(),
                    offense.offense_type(),
                    offense.intent_id(),
                )
                .await?;
                BanAction::PermanentBan
            }
            OffenseSeverity::Moderate => {
                // Moderate offenses use escalating temporary bans
                let offense_count = self.increment_offense_count(pkarr, &offense).await?;
                let action = self.calculate_moderate_action(offense_count);

                match &action {
                    BanAction::TemporaryBan { duration } => {
                        self.add_temporary_ban(
                            pkarr,
                            *duration,
                            offense.to_string(),
                            offense.offense_type(),
                            offense.intent_id(),
                        )
                        .await?;
                    }
                    BanAction::PermanentBan => {
                        self.add_permanent_ban(
                            pkarr,
                            format!("Too many offenses: {}", offense),
                            offense.offense_type(),
                            offense.intent_id(),
                        )
                        .await?;
                    }
                    _ => {}
                }
                action
            }
            OffenseSeverity::Minor => {
                // Minor offenses accumulate warnings before bans
                let warning_count = self.increment_warning_count(pkarr, &offense).await?;

                if warning_count <= 2 {
                    BanAction::Warning
                } else {
                    let duration = Duration::minutes((warning_count as i64 - 2) * 15);
                    self.add_temporary_ban(
                        pkarr,
                        duration,
                        offense.to_string(),
                        offense.offense_type(),
                        offense.intent_id(),
                    )
                    .await?;
                    BanAction::TemporaryBan { duration }
                }
            }
        };

        // Auto-save after recording offense
        let _ = self.save().await;

        Ok(action)
    }

    /// Add a permanent ban
    async fn add_permanent_ban(
        &self,
        pkarr: &str,
        reason: String,
        offense_type: OffenseType,
        intent_id: Option<Uuid>,
    ) -> Result<(), BatchError> {
        let mut bans = self.bans.write().await;

        let ban = BanEntry {
            participant_pkarr: pkarr.to_string(),
            ban_type: BanType::Permanent,
            reason: reason.clone(),
            offense_type,
            first_offense_at: Utc::now(),
            banned_at: Utc::now(),
            expires_at: None,
            offense_count: 1,
            intent_ids: intent_id.map(|id| vec![id]).unwrap_or_default(),
        };

        bans.insert(pkarr.to_string(), ban);
        warn!("Permanently banned participant {} for: {}", pkarr, reason);

        Ok(())
    }

    /// Add a temporary ban
    async fn add_temporary_ban(
        &self,
        pkarr: &str,
        duration: Duration,
        reason: String,
        offense_type: OffenseType,
        intent_id: Option<Uuid>,
    ) -> Result<(), BatchError> {
        let mut bans = self.bans.write().await;

        let expires_at = Utc::now() + duration;

        let ban = BanEntry {
            participant_pkarr: pkarr.to_string(),
            ban_type: BanType::Temporary,
            reason: reason.clone(),
            offense_type,
            first_offense_at: Utc::now(),
            banned_at: Utc::now(),
            expires_at: Some(expires_at),
            offense_count: 1,
            intent_ids: intent_id.map(|id| vec![id]).unwrap_or_default(),
        };

        bans.insert(pkarr.to_string(), ban);
        warn!(
            "Temporarily banned participant {} until {} for: {}",
            pkarr,
            expires_at.format("%Y-%m-%d %H:%M:%S UTC"),
            reason
        );

        Ok(())
    }

    /// Remove expired temporary bans
    pub async fn cleanup_expired_bans(&self) {
        let mut bans = self.bans.write().await;
        let now = Utc::now();

        let expired: Vec<String> = bans
            .iter()
            .filter_map(|(key, ban)| {
                if let BanType::Temporary = &ban.ban_type {
                    if let Some(expires) = ban.expires_at {
                        if expires <= now {
                            return Some(key.clone());
                        }
                    }
                }
                None
            })
            .collect();

        for key in expired {
            if let Some(ban) = bans.remove(&key) {
                info!(
                    "Removed expired ban for participant {}",
                    ban.participant_pkarr
                );
            }
        }
    }

    /// Increment offense count for moderate offenses
    async fn increment_offense_count(
        &self,
        pkarr: &str,
        offense: &Offense,
    ) -> Result<u32, BatchError> {
        let mut bans = self.bans.write().await;

        if let Some(ban) = bans.get_mut(pkarr) {
            ban.offense_count += 1;
            if let Some(id) = offense.intent_id() {
                ban.intent_ids.push(id);
            }
            Ok(ban.offense_count)
        } else {
            // First offense
            Ok(1)
        }
    }

    /// Increment warning count for minor offenses
    async fn increment_warning_count(
        &self,
        pkarr: &str,
        offense: &Offense,
    ) -> Result<u32, BatchError> {
        let mut warnings = self.warnings.write().await;

        let entry = warnings
            .entry(pkarr.to_string())
            .or_insert_with(|| WarningEntry {
                participant_pkarr: pkarr.to_string(),
                offense_type: offense.offense_type(),
                count: 0,
                last_offense_at: Utc::now(),
                intent_ids: Vec::new(),
            });

        entry.count += 1;
        entry.last_offense_at = Utc::now();
        if let Some(id) = offense.intent_id() {
            entry.intent_ids.push(id);
        }

        Ok(entry.count)
    }

    /// Calculate action for moderate offenses based on count
    fn calculate_moderate_action(&self, offense_count: u32) -> BanAction {
        match offense_count {
            1 => BanAction::TemporaryBan {
                duration: Duration::hours(1),
            },
            2 => BanAction::TemporaryBan {
                duration: Duration::hours(6),
            },
            3 => BanAction::TemporaryBan {
                duration: Duration::hours(24),
            },
            4 => BanAction::TemporaryBan {
                duration: Duration::days(7),
            },
            5 => BanAction::TemporaryBan {
                duration: Duration::days(30),
            },
            _ => BanAction::PermanentBan,
        }
    }
}

/// Storage format for ban list persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BanListStorage {
    version: u32,
    last_updated: DateTime<Utc>,
    bans: Vec<BanEntry>,
    warnings: Vec<WarningEntry>,
}

/// Ban entry for a participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanEntry {
    pub participant_pkarr: String,
    pub ban_type: BanType,
    pub reason: String,
    pub offense_type: OffenseType,
    pub first_offense_at: DateTime<Utc>,
    pub banned_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub offense_count: u32,
    pub intent_ids: Vec<Uuid>,
}

/// Warning entry for minor offenses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarningEntry {
    pub participant_pkarr: String,
    pub offense_type: OffenseType,
    pub count: u32,
    pub last_offense_at: DateTime<Utc>,
    pub intent_ids: Vec<Uuid>,
}

/// Type of ban
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BanType {
    Permanent,
    Temporary,
}

/// Types of offenses that can trigger bans
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum OffenseType {
    InvalidCommitment,
    FakeUtxo,
    FailedToReveal,
    FailedToSign,
    InsufficientFunds,
}

/// Offense with context
#[derive(Debug, Clone)]
pub enum Offense {
    InvalidCommitment {
        intent_id: Uuid,
    },
    FakeUtxo {
        intent_id: Uuid,
        outpoint: String,
    },
    FailedToReveal {
        intent_id: Uuid,
    },
    FailedToSign {
        intent_id: Uuid,
    },
    InsufficientFunds {
        intent_id: Uuid,
        available: u64,
        required: u64,
    },
    // Future offense types can be added here as needed:
    // DoubleSpend { intent_id: Uuid },
    // NetworkMismatch { intent_id: Uuid },
    // InvalidRevealData { intent_id: Uuid, reason: String },
    // InvalidSignaturePsbt { intent_id: Uuid },
    // CommitmentSquatting { intent_id: Uuid },
}

impl Offense {
    /// Get the severity level of the offense
    pub fn severity(&self) -> OffenseSeverity {
        match self {
            Self::InvalidCommitment { .. } => OffenseSeverity::Critical,
            Self::FakeUtxo { .. } => OffenseSeverity::Critical,
            Self::InsufficientFunds { .. } => OffenseSeverity::Critical, // Bypassing client-side validation is critical
            Self::FailedToReveal { .. } => OffenseSeverity::Moderate,
            Self::FailedToSign { .. } => OffenseSeverity::Moderate,
        }
    }

    /// Get the offense type
    pub fn offense_type(&self) -> OffenseType {
        match self {
            Self::InvalidCommitment { .. } => OffenseType::InvalidCommitment,
            Self::FakeUtxo { .. } => OffenseType::FakeUtxo,
            Self::InsufficientFunds { .. } => OffenseType::InsufficientFunds,
            Self::FailedToReveal { .. } => OffenseType::FailedToReveal,
            Self::FailedToSign { .. } => OffenseType::FailedToSign,
        }
    }

    /// Get the intent ID if available
    pub fn intent_id(&self) -> Option<Uuid> {
        match self {
            Self::InvalidCommitment { intent_id }
            | Self::FakeUtxo { intent_id, .. }
            | Self::InsufficientFunds { intent_id, .. }
            | Self::FailedToReveal { intent_id }
            | Self::FailedToSign { intent_id } => Some(*intent_id),
        }
    }
}

impl std::fmt::Display for Offense {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCommitment { .. } => write!(f, "Invalid commitment - reveal doesn't match hash"),
            Self::FakeUtxo { outpoint, .. } => write!(f, "Fake UTXO - {} doesn't exist on-chain", outpoint),
            Self::InsufficientFunds { available, required, .. } =>
                write!(f, "Insufficient funds (bypassed client validation) - have {} sats, need at least {} sats", available, required),
            Self::FailedToReveal { .. } => write!(f, "Failed to reveal after commitment"),
            Self::FailedToSign { .. } => write!(f, "Failed to sign after reveal"),
        }
    }
}

/// Severity levels for offenses
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum OffenseSeverity {
    Critical, // Immediate permanent ban
    Moderate, // Escalating temporary bans
    Minor,    // Warnings then temp bans
}

/// Action to take based on offense
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BanAction {
    NoAction,
    Warning,
    TemporaryBan { duration: Duration },
    PermanentBan,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_permanent_ban_critical_offense() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ban_list.json");

        let manager = BanManager::new(path).load().await.unwrap();

        let action = manager
            .record_offense(
                "test_pkarr",
                Offense::InvalidCommitment {
                    intent_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();

        assert!(matches!(action, BanAction::PermanentBan));
        assert!(manager.is_banned("test_pkarr").await);
    }

    #[tokio::test]
    async fn test_escalating_temporary_bans() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ban_list.json");

        let manager = BanManager::new(path).load().await.unwrap();
        let intent_id = Uuid::new_v4();

        // First offense - 1 hour ban
        let action = manager
            .record_offense("test_pkarr", Offense::FailedToReveal { intent_id })
            .await
            .unwrap();

        assert!(
            matches!(action, BanAction::TemporaryBan { duration } if duration == Duration::hours(1))
        );
    }

    #[tokio::test]
    async fn test_insufficient_funds_permanent_ban() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ban_list.json");

        let manager = BanManager::new(path).load().await.unwrap();

        let action = manager
            .record_offense(
                "malicious_participant",
                Offense::InsufficientFunds {
                    intent_id: Uuid::new_v4(),
                    available: 1000,
                    required: 50000,
                },
            )
            .await
            .unwrap();

        // InsufficientFunds is critical - bypassing client validation results in permanent ban
        assert!(matches!(action, BanAction::PermanentBan));
        assert!(manager.is_banned("malicious_participant").await);
    }

    #[tokio::test]
    async fn test_ban_expiration_cleanup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ban_list.json");

        let manager = BanManager::new(path).load().await.unwrap();

        // Add a temporary ban that's already expired
        {
            let mut bans = manager.bans.write().await;
            bans.insert(
                "expired_pkarr".to_string(),
                BanEntry {
                    participant_pkarr: "expired_pkarr".to_string(),
                    ban_type: BanType::Temporary,
                    reason: "Test".to_string(),
                    offense_type: OffenseType::FailedToReveal,
                    first_offense_at: Utc::now() - Duration::hours(2),
                    banned_at: Utc::now() - Duration::hours(2),
                    expires_at: Some(Utc::now() - Duration::hours(1)),
                    offense_count: 1,
                    intent_ids: vec![],
                },
            );
        }

        assert!(!manager.is_banned("expired_pkarr").await); // Expired ban should not be active

        manager.cleanup_expired_bans().await;

        assert!(!manager.is_banned("expired_pkarr").await); // Still not banned after cleanup
    }
}
