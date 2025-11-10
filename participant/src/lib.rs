pub mod participant;

use common::Result;

/// Simple configuration for running a participant
#[derive(Debug, Clone)]
pub struct ParticipantConfig {
    /// Coordinator's pubky
    pub coordinator_pubky: String,
    /// Recovery method for pubky identity: "phrase" or "file"
    pub recovery_method: String,
    /// Recovery value: mnemonic phrase or file path for pubky identity
    pub recovery_value: String,
    /// Password for pubky recovery
    pub password: String,
    /// Bitcoin wallet mnemonic phrase
    pub wallet_mnemonic: String,
    /// Network: "bitcoin", "testnet", "signet", or "regtest"
    pub network: String,
    /// Payments to make: list of (address, amount_sats) tuples
    pub payments: Vec<(String, u64)>,
    /// Specific UTXOs to use (optional): list of (txid, vout) tuples
    pub utxos: Option<Vec<(String, u32)>>,
    /// BIP84 account number (default: 0)
    pub account: Option<u32>,
    /// Exit immediately after signing (don't wait for broadcast)
    pub no_wait: bool,
    /// Specific intent ID to join (optional)
    pub intent_id: Option<String>,
    /// Electrum server host (optional - uses defaults if not provided)
    pub electrum_host: Option<String>,
    /// Electrum server port (optional)
    pub electrum_port: Option<u16>,
    /// Electrum protocol: "tcp" or "ssl" (optional)
    pub electrum_proto: Option<String>,
}

impl Default for ParticipantConfig {
    fn default() -> Self {
        Self {
            coordinator_pubky: String::new(),
            recovery_method: "phrase".to_string(),
            recovery_value: String::new(),
            password: String::new(),
            wallet_mnemonic: String::new(),
            network: "signet".to_string(),
            payments: Vec::new(),
            utxos: None,
            account: None,
            no_wait: false,
            intent_id: None,
            electrum_host: None,
            electrum_port: None,
            electrum_proto: None,
        }
    }
}

/// Run a participant with simple configuration
///
/// # Example
///
/// ```no_run
/// use participant::{run_participant, ParticipantConfig};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = ParticipantConfig {
///         coordinator_pubky: "coordinator_pubky_here".to_string(),
///         recovery_method: "phrase".to_string(),
///         recovery_value: "your pubky twelve word mnemonic".to_string(),
///         password: "password".to_string(),
///         wallet_mnemonic: "your bitcoin wallet mnemonic".to_string(),
///         network: "signet".to_string(),
///         payments: vec![
///             ("tb1qaddress1".to_string(), 50000),
///             ("tb1qaddress2".to_string(), 25000),
///         ],
///         ..Default::default()
///     };
///
///     run_participant(config).await?;
///     Ok(())
/// }
/// ```
pub async fn run_participant(config: ParticipantConfig) -> Result<()> {
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

    // Format payments
    let payments_str = config
        .payments
        .iter()
        .map(|(addr, amount)| format!("{}:{}", addr, amount))
        .collect::<Vec<_>>()
        .join(",");

    // Format UTXOs if provided
    let utxos_str = config.utxos.map(|utxos| {
        utxos
            .iter()
            .map(|(txid, vout)| format!("{}:{}", txid, vout))
            .collect::<Vec<_>>()
            .join(",")
    });

    participant::run(
        &config.recovery_method,
        &config.recovery_value,
        &config.password,
        &config.coordinator_pubky,
        &config.wallet_mnemonic,
        config.account.unwrap_or(0), // account number
        &config.network,
        &payments_str,
        utxos_str.as_deref(),
        config.intent_id.as_deref(),
        config.no_wait,
        &electrum_host,
        electrum_port,
        &electrum_proto,
    )
    .await
}

/// Run a participant with minimal configuration
///
/// # Example
///
/// ```no_run
/// use participant::run_participant_simple;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Join a batch with just the essentials
///     run_participant_simple(
///         "coordinator_pubky_here",
///         "your pubky mnemonic phrase",
///         "password",
///         "your bitcoin wallet mnemonic",
///         "signet",
///         vec![
///             ("tb1qaddress1", 50000),
///             ("tb1qaddress2", 25000),
///         ],
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn run_participant_simple(
    coordinator_pubky: &str,
    pubky_phrase: &str,
    password: &str,
    wallet_mnemonic: &str,
    network: &str,
    payments: Vec<(&str, u64)>,
) -> Result<()> {
    let config = ParticipantConfig {
        coordinator_pubky: coordinator_pubky.to_string(),
        recovery_method: "phrase".to_string(),
        recovery_value: pubky_phrase.to_string(),
        password: password.to_string(),
        wallet_mnemonic: wallet_mnemonic.to_string(),
        network: network.to_string(),
        payments: payments
            .into_iter()
            .map(|(addr, amt)| (addr.to_string(), amt))
            .collect(),
        ..Default::default()
    };

    run_participant(config).await
}

/// Run a participant with automatic UTXO selection
///
/// # Example
///
/// ```no_run
/// use participant::run_participant_auto;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Join a batch and let the library select UTXOs automatically
///     run_participant_auto(
///         "coordinator_pubky_here",
///         "your pubky mnemonic phrase",
///         "password",
///         "your bitcoin wallet mnemonic",
///         "signet",
///         100000,  // total amount to send
///         vec![
///             ("tb1qaddress1", 60000),
///             ("tb1qaddress2", 40000),
///         ],
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn run_participant_auto(
    coordinator_pubky: &str,
    pubky_phrase: &str,
    password: &str,
    wallet_mnemonic: &str,
    network: &str,
    _total_amount: u64, // For future auto-selection implementation
    payments: Vec<(&str, u64)>,
) -> Result<()> {
    // For now, just use the simple version
    // TODO: Implement automatic UTXO selection based on total_amount
    run_participant_simple(
        coordinator_pubky,
        pubky_phrase,
        password,
        wallet_mnemonic,
        network,
        payments,
    )
    .await
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
