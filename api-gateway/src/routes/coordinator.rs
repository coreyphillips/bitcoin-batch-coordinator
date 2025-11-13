use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use crate::state::AppState;
use tracing::{error, info};

#[derive(Serialize)]
pub struct CoordinatorStatusResponse {
    pub running: bool,
    pub configured: bool,
    pub network: Option<String>,
    pub fee_rate: Option<u64>,
    pub min_participants: Option<usize>,
    pub max_participants: Option<usize>,
}

/// Get coordinator status
pub async fn get_coordinator_status(
    State(state): State<AppState>,
) -> Result<Json<CoordinatorStatusResponse>, StatusCode> {
    let coordinator = state.coordinator.read().await;
    let status = coordinator.status().await;

    Ok(Json(CoordinatorStatusResponse {
        running: status.running,
        configured: status.configured,
        network: status.network,
        fee_rate: status.fee_rate,
        min_participants: status.min_participants,
        max_participants: status.max_participants,
    }))
}

// Note: Coordinator start/stop endpoints removed for now.
// The batch coordinator needs to run as a separate process or binary
// due to threading constraints in the pubky-messenger library
// (contains non-Send types that prevent tokio::spawn).
//
// To run the coordinator, use the standalone coordinator binary
// which can read the identity from the database and start the coordinator.
