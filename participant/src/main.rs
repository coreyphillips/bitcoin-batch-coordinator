mod participant;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use std::fs;
use std::path::Path;
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
#[command(name = "participant")]
#[command(about = "Bitcoin batch transaction participant", long_about = None)]
struct Args {
    /// Recovery pkarr key file (or use --pubky-phrase instead)
    #[arg(conflicts_with = "pubky_phrase")]
    recovery_pkarr: Option<String>,

    /// Pubky recovery phrase (12-word mnemonic, alternative to recovery_pkarr file)
    #[arg(long, conflicts_with = "recovery_pkarr")]
    pubky_phrase: Option<String>,

    /// Password/passphrase for recovery key
    #[arg(short, long)]
    pass: String,

    /// Coordinator pkarr (public key string)
    coordinator_pkarr: String,

    /// Bitcoin BIP39 mnemonic (12 or 24 words) for wallet
    #[arg(long)]
    mnemonic: String,

    /// BIP84 account number (default: 0)
    #[arg(long, default_value = "0")]
    account: u32,

    /// Network: signet, testnet, bitcoin, regtest
    #[arg(long, default_value = "signet")]
    network: String,

    /// Payments: addr:amount,addr:amount,...
    #[arg(long)]
    pay: String,

    /// Select UTXOs (optional): txid:vout,txid:vout,...
    #[arg(long)]
    select: Option<String>,

    /// Intent ID (if joining existing batch)
    #[arg(long)]
    intent_id: Option<String>,

    /// Don't wait for final transaction (sign and leave)
    #[arg(long)]
    no_wait: bool,

    /// Electrum server host
    #[arg(long, default_value = "blockstream.info")]
    electrum_host: String,

    /// Electrum server port
    #[arg(long, default_value = "50002")]
    electrum_port: u16,

    /// Electrum protocol: tcp or ssl
    #[arg(long, default_value = "ssl")]
    electrum_proto: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut args = Args::parse();

    // Apply network-specific Electrum defaults if using default values
    if args.electrum_host == "blockstream.info"
        && args.electrum_port == 50002
        && args.electrum_proto == "ssl"
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

    // Validate that either recovery_pkarr or pubky_phrase is provided
    let (recovery_method, recovery_value) = match (&args.recovery_pkarr, &args.pubky_phrase) {
        (Some(pkarr), None) => ("file", pkarr.as_str()),
        (None, Some(phrase)) => ("phrase", phrase.as_str()),
        _ => {
            return Err(anyhow::anyhow!(
                "Must provide either a recovery pkarr file or --pubky-phrase"
            ));
        }
    };

    participant::run(
        recovery_method,
        recovery_value,
        &args.pass,
        &args.coordinator_pkarr,
        &args.mnemonic,
        args.account,
        &args.network,
        &args.pay,
        args.select.as_deref(),
        args.intent_id.as_deref(),
        args.no_wait,
        &args.electrum_host,
        args.electrum_port,
        &args.electrum_proto,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))
}
