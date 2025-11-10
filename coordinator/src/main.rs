mod ban_manager;
mod batch_manager;
mod coordinator;
mod coordinator_v2;

use anyhow::Result;
use ban_manager::BanManager;
use clap::Parser;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing_subscriber;

#[derive(Debug, Deserialize)]
struct ElectrumConfig {
    host: String,
    port: u16,
    protocol: String,
}

#[derive(Debug, Deserialize)]
struct ElectrumServersConfig {
    bitcoin: Option<ElectrumConfig>,
    testnet: Option<ElectrumConfig>,
    signet: Option<ElectrumConfig>,
    regtest: Option<ElectrumConfig>,
}

#[derive(Parser, Debug)]
#[command(name = "coordinator")]
#[command(about = "Bitcoin batch transaction coordinator", long_about = None)]
struct Args {
    /// Recovery pkarr key file (or use --recovery-phrase instead)
    #[arg(conflicts_with = "recovery_phrase")]
    recovery_pkarr: Option<String>,

    /// Recovery phrase (12-word mnemonic, alternative to recovery_pkarr file)
    #[arg(long, conflicts_with = "recovery_pkarr")]
    recovery_phrase: Option<String>,

    /// Password/passphrase for recovery key
    #[arg(short, long)]
    pass: String,

    /// Network: signet, testnet, bitcoin, regtest
    #[arg(long, default_value = "signet")]
    network: String,

    /// Fee rate in sat/vB
    #[arg(long, default_value = "10")]
    fee_rate: u64,

    /// Minimum participants
    #[arg(long, default_value = "2")]
    min: usize,

    /// Maximum participants
    #[arg(long, default_value = "10")]
    max: usize,

    /// Deadline in milliseconds
    #[arg(long, default_value = "60000")]
    deadline_ms: u64,

    /// Disallow change outputs (change outputs are allowed by default)
    #[arg(long)]
    no_allow_change: bool,

    /// Electrum server host
    #[arg(long, default_value = "127.0.0.1")]
    electrum_host: String,

    /// Electrum server port
    #[arg(long, default_value = "50001")]
    electrum_port: u16,

    /// Electrum protocol (tcp or ssl)
    #[arg(long, default_value = "tcp")]
    electrum_proto: String,

    /// Expected participant public keys (comma-separated)
    #[arg(long)]
    participants: Option<String>,

    /// Enable multi-batch mode (rolling batches)
    #[arg(long)]
    multi_batch: bool,

    /// Seed follow list with pubkys (comma-separated) to follow them and who they follow
    #[arg(long)]
    seed_follows: Option<String>,

    /// Broadcast intent to all followers (disabled by default to avoid sending to potentially thousands of users)
    #[arg(long)]
    broadcast_to_followers: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut args = Args::parse();

    // Apply network-specific Electrum defaults if using default values
    if args.electrum_host == "127.0.0.1"
        && args.electrum_port == 50001
        && args.electrum_proto == "tcp"
    {
        // First try to load from config file
        let config_path = Path::new("electrum-servers.toml");
        let config = if config_path.exists() {
            match fs::read_to_string(config_path) {
                Ok(content) => match toml::from_str::<ElectrumServersConfig>(&content) {
                    Ok(config) => Some(config),
                    Err(e) => {
                        tracing::warn!("Failed to parse electrum-servers.toml: {}", e);
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read electrum-servers.toml: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Apply config from file or use built-in defaults
        let network_key = match args.network.as_str() {
            "bitcoin" | "mainnet" => "bitcoin",
            n => n,
        };

        if let Some(config) = config {
            let network_config = match network_key {
                "bitcoin" => config.bitcoin.as_ref(),
                "testnet" => config.testnet.as_ref(),
                "signet" => config.signet.as_ref(),
                "regtest" => config.regtest.as_ref(),
                _ => None,
            };

            if let Some(ec) = network_config {
                args.electrum_host = ec.host.clone();
                args.electrum_port = ec.port;
                args.electrum_proto = ec.protocol.clone();
                tracing::info!(
                    "Using Electrum config from file for network {}",
                    network_key
                );
            }
        } else {
            // Fall back to built-in defaults
            match network_key {
                "bitcoin" => {
                    args.electrum_host = "35.187.18.233".to_string();
                    args.electrum_port = 8900;
                    args.electrum_proto = "ssl".to_string();
                }
                "testnet" => {
                    args.electrum_host = "electrum.blockstream.info".to_string();
                    args.electrum_port = 60002;
                    args.electrum_proto = "ssl".to_string();
                }
                "signet" => {
                    args.electrum_host = "electrum.blockstream.info".to_string();
                    args.electrum_port = 60602;
                    args.electrum_proto = "ssl".to_string();
                }
                "regtest" => {
                    args.electrum_host = "34.65.252.32".to_string();
                    args.electrum_port = 18483;
                    args.electrum_proto = "tcp".to_string();
                }
                _ => {} // Keep defaults for unknown networks
            }
        }
    }

    // Validate that either recovery_pkarr or recovery_phrase is provided
    let (recovery_method, recovery_value) = match (&args.recovery_pkarr, &args.recovery_phrase) {
        (Some(pkarr), None) => ("file", pkarr.as_str()),
        (None, Some(phrase)) => ("phrase", phrase.as_str()),
        _ => {
            return Err(anyhow::anyhow!(
                "Must provide either a recovery pkarr file or --recovery-phrase"
            ));
        }
    };

    // Initialize ban manager
    tracing::info!("Initializing ban list manager...");
    let ban_manager = Arc::new(
        BanManager::new(PathBuf::from("ban_list.json"))
            .load()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to load ban list: {}", e))?,
    );
    tracing::info!("Ban list loaded successfully");

    // Start cleanup task for expired bans
    let cleanup_manager = ban_manager.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(3600)).await; // Every hour
            cleanup_manager.cleanup_expired_bans().await;
            if let Err(e) = cleanup_manager.save().await {
                tracing::error!("Failed to save ban list after cleanup: {}", e);
            }
        }
    });

    if args.multi_batch {
        // Run the multi-batch coordinator (supports rolling batches)
        coordinator::run_multi_batch(
            recovery_method,
            recovery_value,
            &args.pass,
            &args.network,
            args.fee_rate,
            args.min,
            args.max,
            args.deadline_ms,
            !args.no_allow_change, // Invert: no_allow_change flag -> allow_change bool
            &args.electrum_host,
            args.electrum_port,
            &args.electrum_proto,
            args.participants.as_deref(),
            args.seed_follows.as_deref(),
            args.broadcast_to_followers,
            ban_manager,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
    } else {
        // Run the original single-batch coordinator
        coordinator::run(
            recovery_method,
            recovery_value,
            &args.pass,
            &args.network,
            args.fee_rate,
            args.min,
            args.max,
            args.deadline_ms,
            !args.no_allow_change, // Invert: no_allow_change flag -> allow_change bool
            &args.electrum_host,
            args.electrum_port,
            &args.electrum_proto,
            args.participants.as_deref(),
            args.seed_follows.as_deref(),
            args.broadcast_to_followers,
            ban_manager,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
    }
}
