use coordinator::CoordinatorConfig;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use sqlx::SqlitePool;

/// Coordinator runtime state
/// Note: Actual coordinator integration is pending due to threading constraints
/// in the pubky-messenger library (non-Send types across await points)
pub struct CoordinatorManager {
    /// Coordinator configuration
    pub config: Arc<RwLock<Option<CoordinatorConfig>>>,
    /// Database pool for fetching identity
    db: SqlitePool,
}

impl CoordinatorManager {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            config: Arc::new(RwLock::new(None)),
            db,
        }
    }

    /// Check if coordinator is configured (has identity)
    pub async fn is_configured(&self) -> bool {
        let identity = crate::db::identity_queries::get_coordinator_identity(&self.db)
            .await
            .ok()
            .flatten();
        identity.is_some()
    }

    /// Get coordinator status
    pub async fn status(&self) -> CoordinatorStatus {
        let is_configured = self.is_configured().await;
        let config = self.config.read().await.clone();

        CoordinatorStatus {
            running: false, // TODO: Implement after resolving threading issues
            configured: is_configured,
            network: config.as_ref().map(|c| c.network.clone()),
            fee_rate: config.as_ref().map(|c| c.fee_rate),
            min_participants: config.as_ref().map(|c| c.min_participants),
            max_participants: config.as_ref().map(|c| c.max_participants),
        }
    }

    /// Prepare coordinator config (to be used when starting manually)
    pub async fn prepare_config(&self, passphrase: &str) -> Result<CoordinatorConfig, String> {
        // Fetch identity from database
        let identity = crate::db::identity_queries::get_coordinator_identity(&self.db)
            .await
            .map_err(|e| format!("Failed to fetch identity: {}", e))?
            .ok_or("No identity configured")?;

        info!("Preparing coordinator config with identity type: {}", identity.identity_type);

        // Decrypt identity data
        let decrypted_data = crate::crypto::decrypt_data(&identity.encrypted_data, passphrase)
            .map_err(|e| format!("Failed to decrypt identity: {}", e))?;

        // Determine recovery method and value
        let (recovery_method, recovery_value) = match identity.identity_type.as_str() {
            "file" => {
                // For file, write to temporary location
                use std::io::Write;
                let temp_path = "/tmp/coordinator_identity.pkarr";
                let mut file = std::fs::File::create(temp_path)
                    .map_err(|e| format!("Failed to create temp file: {}", e))?;
                file.write_all(&decrypted_data)
                    .map_err(|e| format!("Failed to write temp file: {}", e))?;
                ("file", temp_path.to_string())
            }
            "phrase" => {
                let phrase = String::from_utf8(decrypted_data)
                    .map_err(|e| format!("Invalid UTF-8 in phrase: {}", e))?;
                ("phrase", phrase)
            }
            _ => return Err(format!("Unknown identity type: {}", identity.identity_type)),
        };

        // Create default config
        let config = CoordinatorConfig {
            recovery_method: recovery_method.to_string(),
            recovery_value,
            password: passphrase.to_string(),
            network: "signet".to_string(), // Default to signet
            fee_rate: 10,
            min_participants: 2,
            max_participants: 10,
            deadline_ms: 300000, // 5 minutes
            allow_change: true,
            multi_batch: true,
            broadcast_to_followers: false,
            electrum_host: None,
            electrum_port: None,
            electrum_proto: None,
            participants: None,
            seed_follows: None,
        };

        // Store config
        {
            let mut cfg = self.config.write().await;
            *cfg = Some(config.clone());
        }

        Ok(config)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CoordinatorStatus {
    pub running: bool,
    pub configured: bool,
    pub network: Option<String>,
    pub fee_rate: Option<u64>,
    pub min_participants: Option<usize>,
    pub max_participants: Option<usize>,
}
