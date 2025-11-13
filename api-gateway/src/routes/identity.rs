use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use crate::{db::identity_queries, state::AppState};
use tracing::{error, info};

#[derive(Serialize)]
pub struct IdentityResponse {
    pub pubkey: String,
    pub identity_type: String,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct GenerateIdentityResponse {
    pub pubkey: String,
    pub recovery_phrase: String,
    pub message: String,
}

#[derive(Deserialize)]
pub struct ImportPhraseRequest {
    pub recovery_phrase: String,
    pub passphrase: String,
}

#[derive(Deserialize)]
pub struct ExportIdentityRequest {
    pub passphrase: String,
}

#[derive(Serialize)]
pub struct ExportIdentityResponse {
    pub identity_type: String,
    pub data: String, // Base64 encoded for file, or plain phrase
    pub pubkey: String,
}

/// Get current coordinator identity
pub async fn get_current_identity(
    State(state): State<AppState>,
) -> Result<Json<Option<IdentityResponse>>, StatusCode> {
    let identity = identity_queries::get_coordinator_identity(&state.db)
        .await
        .map_err(|e| {
            error!("Failed to get identity: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(identity.map(|i| IdentityResponse {
        pubkey: i.pubkey,
        identity_type: i.identity_type,
        created_at: i.created_at,
    })))
}

/// Import identity from .pkarr file
pub async fn import_from_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IdentityResponse>, StatusCode> {
    let mut file_data: Option<Vec<u8>> = None;
    let mut passphrase: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        error!("Multipart error: {}", e);
        StatusCode::BAD_REQUEST
    })? {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                file_data = Some(field.bytes().await.map_err(|e| {
                    error!("Failed to read file: {}", e);
                    StatusCode::BAD_REQUEST
                })?.to_vec());
            }
            "passphrase" => {
                passphrase = Some(field.text().await.map_err(|e| {
                    error!("Failed to read passphrase: {}", e);
                    StatusCode::BAD_REQUEST
                })?);
            }
            _ => {}
        }
    }

    let file_data = file_data.ok_or_else(|| {
        error!("No file uploaded");
        StatusCode::BAD_REQUEST
    })?;

    let passphrase = passphrase.ok_or_else(|| {
        error!("No passphrase provided");
        StatusCode::BAD_REQUEST
    })?;

    // Extract pubkey from file using pubky-messenger
    // For now, we'll use a placeholder - this needs integration with pubky-messenger
    let pubkey = extract_pubkey_from_file(&file_data, &passphrase).map_err(|e| {
        error!("Failed to extract pubkey: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    // Save to database
    identity_queries::save_coordinator_identity(&state.db, "file", &file_data, &pubkey)
        .await
        .map_err(|e| {
            error!("Failed to save identity: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("Identity imported from file: {}", pubkey);

    Ok(Json(IdentityResponse {
        pubkey,
        identity_type: "file".to_string(),
        created_at: chrono::Utc::now().timestamp(),
    }))
}

/// Import identity from recovery phrase
pub async fn import_from_phrase(
    State(state): State<AppState>,
    Json(payload): Json<ImportPhraseRequest>,
) -> Result<Json<IdentityResponse>, StatusCode> {
    // Validate recovery phrase
    let words: Vec<&str> = payload.recovery_phrase.trim().split_whitespace().collect();
    if words.len() != 12 && words.len() != 24 {
        error!("Invalid recovery phrase length: {}", words.len());
        return Err(StatusCode::BAD_REQUEST);
    }

    // Extract pubkey from phrase using pubky-messenger
    let pubkey = extract_pubkey_from_phrase(&payload.recovery_phrase, &payload.passphrase)
        .map_err(|e| {
            error!("Failed to extract pubkey from phrase: {}", e);
            StatusCode::BAD_REQUEST
        })?;

    // Encrypt and save the phrase
    let encrypted_data = encrypt_data(payload.recovery_phrase.as_bytes(), &payload.passphrase)
        .map_err(|e| {
            error!("Failed to encrypt phrase: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    identity_queries::save_coordinator_identity(&state.db, "phrase", &encrypted_data, &pubkey)
        .await
        .map_err(|e| {
            error!("Failed to save identity: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("Identity imported from phrase: {}", pubkey);

    Ok(Json(IdentityResponse {
        pubkey,
        identity_type: "phrase".to_string(),
        created_at: chrono::Utc::now().timestamp(),
    }))
}

/// Generate new identity
pub async fn generate_identity(
    State(state): State<AppState>,
    Json(payload): Json<ExportIdentityRequest>,
) -> Result<Json<GenerateIdentityResponse>, StatusCode> {
    // Generate new mnemonic using pubky-messenger
    let (pubkey, recovery_phrase) = generate_new_identity().map_err(|e| {
        error!("Failed to generate identity: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Encrypt and save
    let encrypted_data = encrypt_data(recovery_phrase.as_bytes(), &payload.passphrase)
        .map_err(|e| {
            error!("Failed to encrypt phrase: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    identity_queries::save_coordinator_identity(&state.db, "generated", &encrypted_data, &pubkey)
        .await
        .map_err(|e| {
            error!("Failed to save identity: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("New identity generated: {}", pubkey);

    Ok(Json(GenerateIdentityResponse {
        pubkey,
        recovery_phrase,
        message: "IMPORTANT: Save this recovery phrase in a secure location!".to_string(),
    }))
}

/// Delete identity
pub async fn delete_identity(
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    identity_queries::delete_coordinator_identity(&state.db)
        .await
        .map_err(|e| {
            error!("Failed to delete identity: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("Identity deleted");
    Ok(StatusCode::NO_CONTENT)
}

// Helper functions - these need proper integration with pubky-messenger

fn extract_pubkey_from_file(_file_data: &[u8], _passphrase: &str) -> Result<String, String> {
    // TODO: Integrate with pubky-messenger to extract pubkey from .pkarr file
    // This is a placeholder
    Err("Not yet implemented - requires pubky-messenger integration".to_string())
}

fn extract_pubkey_from_phrase(_phrase: &str, _passphrase: &str) -> Result<String, String> {
    // TODO: Integrate with pubky-messenger to derive pubkey from mnemonic
    // This is a placeholder
    Err("Not yet implemented - requires pubky-messenger integration".to_string())
}

fn generate_new_identity() -> Result<(String, String), String> {
    // TODO: Integrate with pubky-messenger to generate new identity
    // Should return (pubkey, recovery_phrase)
    // This is a placeholder
    Err("Not yet implemented - requires pubky-messenger integration".to_string())
}

fn encrypt_data(_data: &[u8], _passphrase: &str) -> Result<Vec<u8>, String> {
    // TODO: Implement proper encryption (AES-256-GCM or similar)
    // For now, just return the data as-is (INSECURE - placeholder only)
    Ok(_data.to_vec())
}

fn _decrypt_data(_encrypted: &[u8], _passphrase: &str) -> Result<Vec<u8>, String> {
    // TODO: Implement proper decryption
    // For now, just return the data as-is (INSECURE - placeholder only)
    Ok(_encrypted.to_vec())
}
