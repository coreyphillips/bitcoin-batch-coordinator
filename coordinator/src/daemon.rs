use anyhow::Result;
use clap::Parser;
use coordinator::{run_coordinator, CoordinatorConfig};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::io::{self, Write};
use std::str::FromStr;
use tracing::{error, info};

// Crypto imports for decryption
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Database URL
    #[arg(long, default_value = "sqlite:///data/coordinator.db")]
    database_url: String,

    /// Passphrase for decrypting identity (will prompt if not provided)
    #[arg(long)]
    passphrase: Option<String>,

    /// Network: bitcoin, testnet, signet, or regtest
    #[arg(long, default_value = "regtest")]
    network: String,

    /// Fee rate in sat/vB
    #[arg(long, default_value = "10")]
    fee_rate: u64,

    /// Minimum participants
    #[arg(long, default_value = "2")]
    min_participants: usize,

    /// Maximum participants
    #[arg(long, default_value = "10")]
    max_participants: usize,

    /// Deadline in milliseconds
    #[arg(long, default_value = "300000")]
    deadline_ms: u64,

    /// Allow change outputs
    #[arg(long, default_value = "true")]
    allow_change: bool,

    /// Enable multi-batch mode
    #[arg(long, default_value = "true")]
    multi_batch: bool,

    /// Broadcast to followers
    #[arg(long)]
    broadcast_to_followers: bool,
}

#[derive(sqlx::FromRow)]
struct CoordinatorIdentity {
    identity_type: String,
    encrypted_data: Vec<u8>,
    pubkey: String,
}

#[derive(sqlx::FromRow)]
struct CoordinatorConfigRow {
    network: String,
    fee_rate: i64,
    min_participants: i64,
    max_participants: i64,
    deadline_ms: i64,
    allow_change: bool,
    multi_batch: bool,
    broadcast_to_followers: bool,
    encrypted_passphrase: Option<Vec<u8>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    info!("🚀 Starting Bitcoin Batch Coordinator Daemon");
    info!("   Version: {}", env!("CARGO_PKG_VERSION"));
    info!("   Database: {}", args.database_url);
    info!("   Network: {}", args.network);

    // Connect to database
    info!("📦 Connecting to database...");
    let options = SqliteConnectOptions::from_str(&args.database_url)?
        .create_if_missing(false); // Don't create, must exist

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    info!("✅ Database connected");

    // Fetch coordinator config from database
    info!("⚙️  Loading coordinator configuration...");
    let config_row: Option<CoordinatorConfigRow> = sqlx::query_as(
        "SELECT network, fee_rate, min_participants, max_participants, deadline_ms,
                allow_change, multi_batch, broadcast_to_followers, encrypted_passphrase
         FROM coordinator_config
         WHERE id = 1"
    )
    .fetch_optional(&pool)
    .await?;

    let config_row = config_row.ok_or_else(|| {
        anyhow::anyhow!("No coordinator configuration found. Database may be corrupted.")
    })?;

    // Use database config, but CLI args override
    let network = args.network;
    let fee_rate = args.fee_rate;
    let min_participants = args.min_participants;
    let max_participants = args.max_participants;
    let deadline_ms = args.deadline_ms;
    let allow_change = args.allow_change;
    let multi_batch = args.multi_batch;
    let broadcast_to_followers = args.broadcast_to_followers;

    info!("✅ Configuration loaded (network: {}, fee: {} sat/vB)", network, fee_rate);

    // Fetch identity from database
    info!("🔑 Fetching coordinator identity...");
    let identity: Option<CoordinatorIdentity> = sqlx::query_as(
        "SELECT identity_type, encrypted_data, pubkey
         FROM coordinator_identity
         WHERE is_active = 1
         LIMIT 1"
    )
    .fetch_optional(&pool)
    .await?;

    let identity = identity.ok_or_else(|| {
        anyhow::anyhow!("No coordinator identity found in database. Please import an identity via the web dashboard first.")
    })?;

    info!("✅ Found {} identity: {}", identity.identity_type, identity.pubkey);

    // Get passphrase - try database first, then CLI, then prompt
    let passphrase = if let Some(pass) = args.passphrase {
        info!("Using passphrase from CLI argument");
        pass
    } else if let Some(encrypted_pass) = config_row.encrypted_passphrase {
        // Decrypt passphrase from database
        info!("🔓 Decrypting stored passphrase...");
        let decrypted_pass = decrypt_data(&encrypted_pass, "coordinator-internal-key")
            .map_err(|e| anyhow::anyhow!("Failed to decrypt stored passphrase: {}", e))?;
        String::from_utf8(decrypted_pass)?
    } else {
        // Prompt for passphrase
        print!("🔐 Enter passphrase to decrypt identity: ");
        io::stdout().flush()?;
        let mut passphrase = String::new();
        io::stdin().read_line(&mut passphrase)?;
        passphrase.trim().to_string()
    };

    // Decrypt identity
    info!("🔓 Decrypting identity...");
    let decrypted_data = decrypt_data(&identity.encrypted_data, &passphrase)
        .map_err(|e| anyhow::anyhow!("Failed to decrypt identity: {}", e))?;

    info!("✅ Identity decrypted successfully");

    // Prepare recovery method and value
    let (recovery_method, recovery_value) = match identity.identity_type.as_str() {
        "file" => {
            // Write decrypted data to temporary file
            let temp_path = "/tmp/coordinator_identity.pkarr";
            std::fs::write(temp_path, &decrypted_data)?;
            info!("📄 Wrote identity to temporary file: {}", temp_path);
            ("file", temp_path.to_string())
        }
        "phrase" => {
            let phrase = String::from_utf8(decrypted_data)?;
            info!("📝 Using recovery phrase");
            ("phrase", phrase)
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown identity type: {}",
                identity.identity_type
            ))
        }
    };

    // Create coordinator config
    let config = CoordinatorConfig {
        recovery_method: recovery_method.to_string(),
        recovery_value,
        password: passphrase,
        network,
        fee_rate,
        min_participants,
        max_participants,
        deadline_ms,
        allow_change,
        multi_batch,
        broadcast_to_followers,
        electrum_host: None,
        electrum_port: None,
        electrum_proto: None,
        participants: None,
        seed_follows: None,
    };

    info!("🎯 Starting coordinator with identity: {}", identity.pubkey);
    info!("   Network: {}", config.network);
    info!("   Fee rate: {} sat/vB", config.fee_rate);
    info!("   Participants: {}-{}", config.min_participants, config.max_participants);
    info!("   Deadline: {}ms", config.deadline_ms);
    info!("   Multi-batch: {}", config.multi_batch);

    // Run coordinator
    match run_coordinator(config).await {
        Ok(_) => {
            info!("✅ Coordinator completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("❌ Coordinator error: {}", e);
            Err(e.into())
        }
    }
}

// Decryption functions (same as api-gateway/crypto.rs)

const NONCE_SIZE: usize = 12;

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let argon2 = Argon2::default();
    let salt_str = SaltString::encode_b64(salt)
        .map_err(|e| format!("Failed to encode salt: {}", e))?;

    let password_hash = argon2
        .hash_password(passphrase.as_bytes(), &salt_str)
        .map_err(|e| format!("Failed to hash password: {}", e))?;

    let hash = password_hash.hash.ok_or("No hash generated")?;
    let hash_bytes = hash.as_bytes();

    let mut key = [0u8; 32];
    let len = std::cmp::min(hash_bytes.len(), 32);
    key[..len].copy_from_slice(&hash_bytes[..len]);

    Ok(key)
}

fn decrypt_data(encrypted: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    if encrypted.len() < 16 + NONCE_SIZE {
        return Err("Invalid encrypted data: too short".to_string());
    }

    // Extract salt, nonce, and ciphertext
    let salt = &encrypted[..16];
    let nonce_bytes = &encrypted[16..16 + NONCE_SIZE];
    let ciphertext = &encrypted[16 + NONCE_SIZE..];

    // Derive key from passphrase
    let key = derive_key(passphrase, salt)?;

    // Create cipher
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(plaintext)
}
