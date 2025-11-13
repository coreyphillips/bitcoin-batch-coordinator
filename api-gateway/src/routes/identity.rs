use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use crate::{db::identity_queries, state::AppState, crypto};
use tracing::{error, info};
use pubky_messenger::PrivateMessengerClient;

#[derive(Serialize)]
pub struct IdentityResponse {
    pub pubkey: String,
    pub identity_type: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct ImportPhraseRequest {
    pub recovery_phrase: String,
    pub passphrase: String,
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
    let pubkey = extract_pubkey_from_file(&file_data, &passphrase).map_err(|e| {
        error!("Failed to extract pubkey: {}", e);
        StatusCode::BAD_REQUEST
    })?;

    // Encrypt the file data before storing
    let encrypted_data = crypto::encrypt_data(&file_data, &passphrase).map_err(|e| {
        error!("Failed to encrypt file data: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Save to database
    identity_queries::save_coordinator_identity(&state.db, "file", &encrypted_data, &pubkey)
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
    let encrypted_data = crypto::encrypt_data(payload.recovery_phrase.as_bytes(), &payload.passphrase)
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

// Helper functions using pubky-messenger

fn extract_pubkey_from_file(file_data: &[u8], passphrase: &str) -> Result<String, String> {
    // Create messenger client from recovery file
    let messenger = PrivateMessengerClient::from_recovery_file(file_data, Some(passphrase))
        .map_err(|e| format!("Failed to load recovery file: {}", e))?;

    // Get the public key
    Ok(messenger.public_key_string())
}

fn extract_pubkey_from_phrase(phrase: &str, _passphrase: &str) -> Result<String, String> {
    // Create messenger client from recovery phrase
    // Note: pubky-messenger uses the phrase itself, not an additional passphrase for derivation
    let messenger = PrivateMessengerClient::from_recovery_phrase(phrase, None, None)
        .map_err(|e| format!("Failed to load recovery phrase: {}", e))?;

    // Get the public key
    Ok(messenger.public_key_string())
}
