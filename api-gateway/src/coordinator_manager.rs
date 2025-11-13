use std::sync::Arc;
use std::process::{Child, Command, Stdio};
use tokio::sync::RwLock;
use tracing::info;
use sqlx::SqlitePool;

/// Coordinator runtime state
pub struct CoordinatorManager {
    /// Running coordinator process
    process: Arc<RwLock<Option<Child>>>,
    /// Database pool for fetching identity and config
    db: SqlitePool,
}

impl CoordinatorManager {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            process: Arc::new(RwLock::new(None)),
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

    /// Check if coordinator is running
    pub async fn is_running(&self) -> bool {
        let mut process = self.process.write().await;
        if let Some(child) = process.as_mut() {
            // Check if process is still alive
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited
                    *process = None;
                    false
                }
                Ok(None) => {
                    // Process is still running
                    true
                }
                Err(_) => {
                    // Error checking, assume not running
                    *process = None;
                    false
                }
            }
        } else {
            false
        }
    }

    /// Start the coordinator with stored identity and config
    pub async fn start_coordinator(&self) -> Result<String, String> {
        // Check if already running
        if self.is_running().await {
            return Err("Coordinator is already running".to_string());
        }

        // Fetch identity
        let identity = crate::db::identity_queries::get_coordinator_identity(&self.db)
            .await
            .map_err(|e| format!("Failed to fetch identity: {}", e))?
            .ok_or("No identity configured. Please import an identity first.")?;

        // Fetch config from database
        #[derive(sqlx::FromRow)]
        struct ConfigRow {
            network: String,
            fee_rate: i64,
            min_participants: i64,
            max_participants: i64,
            deadline_ms: i64,
            encrypted_passphrase: Option<Vec<u8>>,
        }

        let config: ConfigRow = sqlx::query_as(
            "SELECT network, fee_rate, min_participants, max_participants, deadline_ms, encrypted_passphrase
             FROM coordinator_config WHERE id = 1"
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| format!("Failed to fetch config: {}", e))?;

        // Decrypt passphrase
        let passphrase = if let Some(encrypted) = config.encrypted_passphrase {
            let decrypted = crate::crypto::decrypt_data(&encrypted, "coordinator-internal-key")
                .map_err(|e| format!("Failed to decrypt passphrase: {}", e))?;
            String::from_utf8(decrypted)
                .map_err(|e| format!("Invalid passphrase encoding: {}", e))?
        } else {
            return Err("No passphrase stored. Please re-import your identity.".to_string());
        };

        // Decrypt identity data
        let decrypted_identity = crate::crypto::decrypt_data(&identity.encrypted_data, &passphrase)
            .map_err(|e| format!("Failed to decrypt identity: {}", e))?;

        // Write identity to temp file
        let temp_path = format!("/tmp/coordinator_identity_{}.pkarr", std::process::id());
        std::fs::write(&temp_path, &decrypted_identity)
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        info!("Starting coordinator with network: {}, participants: {}-{}",
            config.network, config.min_participants, config.max_participants);

        // Get coordinator binary path
        let coordinator_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("coordinator")))
            .ok_or("Failed to find coordinator binary")?;

        // Create log file for coordinator output
        let log_path = "/tmp/coordinator.log";
        let log_file = std::fs::File::create(log_path)
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        // Spawn coordinator process
        let child = Command::new(coordinator_bin)
            .arg(&temp_path)
            .arg("--pass")
            .arg(&passphrase)
            .arg("--network")
            .arg(&config.network)
            .arg("--fee-rate")
            .arg(config.fee_rate.to_string())
            .arg("--min")
            .arg(config.min_participants.to_string())
            .arg("--max")
            .arg(config.max_participants.to_string())
            .arg("--deadline-ms")
            .arg(config.deadline_ms.to_string())
            .arg("--multi-batch")
            .stdout(log_file.try_clone().map_err(|e| format!("Failed to clone log file: {}", e))?)
            .stderr(log_file)
            .spawn()
            .map_err(|e| format!("Failed to spawn coordinator: {}", e))?;

        let pid = child.id();
        info!("Coordinator started with PID: {} (logs: {})", pid, log_path);

        // Store process
        let mut process = self.process.write().await;
        *process = Some(child);

        Ok(format!("Coordinator started (PID: {}). Check logs at {} for batch IDs.", pid, log_path))
    }

    /// Stop the coordinator
    pub async fn stop_coordinator(&self) -> Result<String, String> {
        let mut process = self.process.write().await;
        if let Some(mut child) = process.take() {
            child.kill().map_err(|e| format!("Failed to kill process: {}", e))?;
            child.wait().map_err(|e| format!("Failed to wait for process: {}", e))?;
            info!("Coordinator stopped");
            Ok("Coordinator stopped successfully".to_string())
        } else {
            Err("Coordinator is not running".to_string())
        }
    }

    /// Get coordinator status
    pub async fn status(&self) -> CoordinatorStatus {
        let is_configured = self.is_configured().await;
        let is_running = self.is_running().await;

        // Fetch config from database
        #[derive(sqlx::FromRow)]
        struct ConfigRow {
            network: String,
            fee_rate: i64,
            min_participants: i64,
            max_participants: i64,
        }

        let config: Option<ConfigRow> = sqlx::query_as(
            "SELECT network, fee_rate, min_participants, max_participants FROM coordinator_config WHERE id = 1"
        )
        .fetch_optional(&self.db)
        .await
        .ok()
        .flatten();

        CoordinatorStatus {
            running: is_running,
            configured: is_configured,
            network: config.as_ref().map(|c| c.network.clone()),
            fee_rate: config.as_ref().map(|c| c.fee_rate as u64),
            min_participants: config.as_ref().map(|c| c.min_participants as usize),
            max_participants: config.as_ref().map(|c| c.max_participants as usize),
        }
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
