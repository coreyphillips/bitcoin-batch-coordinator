use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::{db::queries, state::AppState};

#[derive(Serialize)]
pub struct ConfigResponse {
    pub config: HashMap<String, String>,
}

/// Get current configuration
pub async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    // Get all important config values
    let mut config = HashMap::new();

    // Try to get each config value, use defaults if not found
    if let Ok(Some(net)) = queries::get_config(&state.db, "network").await {
        config.insert("network".to_string(), net.value);
    } else {
        config.insert("network".to_string(), "bitcoin".to_string());
    }

    if let Ok(Some(min)) = queries::get_config(&state.db, "min_participants").await {
        config.insert("min_participants".to_string(), min.value);
    } else {
        config.insert("min_participants".to_string(), "2".to_string());
    }

    if let Ok(Some(max)) = queries::get_config(&state.db, "max_participants").await {
        config.insert("max_participants".to_string(), max.value);
    } else {
        config.insert("max_participants".to_string(), "10".to_string());
    }

    if let Ok(Some(timeout)) = queries::get_config(&state.db, "timeout_seconds").await {
        config.insert("timeout_seconds".to_string(), timeout.value);
    } else {
        config.insert("timeout_seconds".to_string(), "300".to_string());
    }

    Ok(Json(ConfigResponse { config }))
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub config: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct UpdateConfigResponse {
    pub message: String,
}

/// Update configuration
pub async fn update_config(
    State(state): State<AppState>,
    Json(payload): Json<UpdateConfigRequest>,
) -> Result<Json<UpdateConfigResponse>, StatusCode> {
    // Update each config value
    for (key, value) in payload.config {
        queries::set_config(&state.db, &key, &value)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(UpdateConfigResponse {
        message: "Configuration updated successfully".to_string(),
    }))
}
