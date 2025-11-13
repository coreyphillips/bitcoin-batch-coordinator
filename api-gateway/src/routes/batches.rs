use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use crate::{db::queries, db::schema::Batch, state::AppState};

#[derive(Deserialize)]
pub struct ListBatchesQuery {
    state: Option<String>,
}

#[derive(Serialize)]
pub struct ListBatchesResponse {
    batches: Vec<Batch>,
    total: usize,
}

/// List all batches, optionally filtered by state
pub async fn list_batches(
    State(state): State<AppState>,
    Query(query): Query<ListBatchesQuery>,
) -> Result<Json<ListBatchesResponse>, StatusCode> {
    let batches = queries::get_batches(&state.db, query.state.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = batches.len();

    Ok(Json(ListBatchesResponse { batches, total }))
}

/// Get a single batch by ID
pub async fn get_batch(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Batch>, StatusCode> {
    let batch = queries::get_batch_by_id(&state.db, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(batch))
}

#[derive(Deserialize)]
pub struct CreateBatchRequest {
    pub min_participants: u32,
    pub max_participants: u32,
    pub timeout_seconds: u64,
}

#[derive(Serialize)]
pub struct CreateBatchResponse {
    pub id: String,
    pub message: String,
}

/// Create a new batch (starts the coordinator)
pub async fn create_batch(
    State(state): State<AppState>,
    Json(_payload): Json<CreateBatchRequest>,
) -> Result<Json<CreateBatchResponse>, StatusCode> {
    // Start the coordinator (will use stored identity and config)
    // The coordinator automatically creates batches when it starts
    let coordinator = state.coordinator.read().await;

    match coordinator.start_coordinator().await {
        Ok(message) => {
            Ok(Json(CreateBatchResponse {
                id: "coordinator-started".to_string(),
                message: format!("{} Coordinator will create batches automatically.", message),
            }))
        }
        Err(e) => {
            if e.contains("already running") {
                Ok(Json(CreateBatchResponse {
                    id: "coordinator-running".to_string(),
                    message: "Coordinator is already running and managing batches".to_string(),
                }))
            } else {
                tracing::error!("Failed to start coordinator: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

#[derive(Serialize)]
pub struct ListParticipantsResponse {
    pub participants: Vec<crate::db::schema::BatchParticipant>,
    pub total: usize,
}

/// List participants for a batch
pub async fn list_participants(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ListParticipantsResponse>, StatusCode> {
    let participants = queries::get_batch_participants(&state.db, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = participants.len();

    Ok(Json(ListParticipantsResponse {
        participants,
        total,
    }))
}
