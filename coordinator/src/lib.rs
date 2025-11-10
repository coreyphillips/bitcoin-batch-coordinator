pub mod ban_manager;
pub mod batch_manager;
pub mod coordinator;
pub mod coordinator_v2;

use ban_manager::BanManager;
use common::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// Simple configuration for running a coordinator
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// Recovery method: "phrase" or "file"
    pub recovery_method: String,
    /// Recovery value: mnemonic phrase or file path
    pub recovery_value: String,
    /// Password for recovery
    pub password: String,
    /// Network: "bitcoin", "testnet", "signet", or "regtest"
    pub network: String,
    /// Fee rate in sat/vB
    pub fee_rate: u64,
    /// Minimum participants
    pub min_participants: usize,
    /// Maximum participants
    pub max_participants: usize,
    /// Deadline in milliseconds
    pub deadline_ms: u64,
    /// Allow change outputs
    pub allow_change: bool,
    /// Electrum server host (optional - uses defaults if not provided)
    pub electrum_host: Option<String>,
    /// Electrum server port (optional)
    pub electrum_port: Option<u16>,
    /// Electrum protocol: "tcp" or "ssl" (optional)
    pub electrum_proto: Option<String>,
    /// Expected participants (optional comma-separated list)
    pub participants: Option<String>,
    /// Seed follows (optional comma-separated list)
    pub seed_follows: Option<String>,
    /// Broadcast to followers
    pub broadcast_to_followers: bool,
    /// Enable multi-batch mode
    pub multi_batch: bool,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            recovery_method: "phrase".to_string(),
            recovery_value: String::new(),
            password: String::new(),
            network: "signet".to_string(),
            fee_rate: 10,
            min_participants: 2,
            max_participants: 10,
            deadline_ms: 60000,
            allow_change: true,
            electrum_host: None,
            electrum_port: None,
            electrum_proto: None,
            participants: None,
            seed_follows: None,
            broadcast_to_followers: false,
            multi_batch: false,
        }
    }
}

/// Run a coordinator with simple configuration
///
/// # Example
///
/// ```no_run
/// use coordinator::{run_coordinator, CoordinatorConfig};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = CoordinatorConfig {
///         recovery_method: "phrase".to_string(),
///         recovery_value: "your twelve word mnemonic phrase here".to_string(),
///         password: "password".to_string(),
///         network: "signet".to_string(),
///         fee_rate: 10,
///         min_participants: 2,
///         max_participants: 5,
///         ..Default::default()
///     };
///
///     run_coordinator(config).await?;
///     Ok(())
/// }
/// ```
pub async fn run_coordinator(config: CoordinatorConfig) -> Result<()> {
    // Get default Electrum settings based on network if not provided
    let (electrum_host, electrum_port, electrum_proto) = if config.electrum_host.is_none() {
        get_default_electrum_config(&config.network)
    } else {
        (
            config
                .electrum_host
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            config.electrum_port.unwrap_or(50001),
            config.electrum_proto.unwrap_or_else(|| "tcp".to_string()),
        )
    };

    // Initialize ban manager (library version starts fresh each time)
    let ban_manager = Arc::new(BanManager::new(PathBuf::from("ban_list.json")));

    // Run the appropriate coordinator
    if config.multi_batch {
        coordinator::run_multi_batch(
            &config.recovery_method,
            &config.recovery_value,
            &config.password,
            &config.network,
            config.fee_rate,
            config.min_participants,
            config.max_participants,
            config.deadline_ms,
            config.allow_change,
            &electrum_host,
            electrum_port,
            &electrum_proto,
            config.participants.as_deref(),
            config.seed_follows.as_deref(),
            config.broadcast_to_followers,
            ban_manager,
        )
        .await
    } else {
        coordinator::run(
            &config.recovery_method,
            &config.recovery_value,
            &config.password,
            &config.network,
            config.fee_rate,
            config.min_participants,
            config.max_participants,
            config.deadline_ms,
            config.allow_change,
            &electrum_host,
            electrum_port,
            &electrum_proto,
            config.participants.as_deref(),
            config.seed_follows.as_deref(),
            config.broadcast_to_followers,
            ban_manager,
        )
        .await
    }
}

/// Run a simple coordinator with minimal configuration
///
/// # Example
///
/// ```no_run
/// use coordinator::run_coordinator_simple;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Run a coordinator with just the essentials
///     run_coordinator_simple(
///         "your twelve word mnemonic phrase",
///         "password",
///         "signet",
///         10,  // fee rate
///         2,   // min participants
///         5,   // max participants
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn run_coordinator_simple(
    recovery_phrase: &str,
    password: &str,
    network: &str,
    fee_rate: u64,
    min_participants: usize,
    max_participants: usize,
) -> Result<()> {
    let config = CoordinatorConfig {
        recovery_method: "phrase".to_string(),
        recovery_value: recovery_phrase.to_string(),
        password: password.to_string(),
        network: network.to_string(),
        fee_rate,
        min_participants,
        max_participants,
        ..Default::default()
    };

    run_coordinator(config).await
}

fn get_default_electrum_config(network: &str) -> (String, u16, String) {
    match network {
        "bitcoin" => ("blockstream.info".to_string(), 700, "ssl".to_string()),
        "testnet" => ("blockstream.info".to_string(), 60001, "ssl".to_string()),
        "signet" => ("mempool.space".to_string(), 60602, "ssl".to_string()),
        "regtest" => ("127.0.0.1".to_string(), 50001, "tcp".to_string()),
        _ => ("127.0.0.1".to_string(), 50001, "tcp".to_string()),
    }
}
