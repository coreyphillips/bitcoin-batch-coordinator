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

/// Create a new batch
pub async fn create_batch(
    State(state): State<AppState>,
    Json(payload): Json<CreateBatchRequest>,
) -> Result<Json<CreateBatchResponse>, StatusCode> {
    // Start the coordinator (will use stored identity and config)
    let coordinator = state.coordinator.read().await;

    match coordinator.start_coordinator().await {
        Ok(message) => {
            // Create a batch record in the database
            let batch_id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().timestamp();

            // Fetch coordinator config for intent data
            #[derive(sqlx::FromRow)]
            struct ConfigRow {
                network: String,
                fee_rate: i64,
            }

            let config: ConfigRow = sqlx::query_as(
                "SELECT network, fee_rate FROM coordinator_config WHERE id = 1"
            )
            .fetch_one(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Create intent data JSON
            let intent_data = serde_json::json!({
                "network": config.network,
                "fee_rate": config.fee_rate,
                "min_participants": payload.min_participants,
                "max_participants": payload.max_participants,
                "deadline_ms": payload.timeout_seconds * 1000,
            }).to_string();

            // Insert batch record
            sqlx::query(
                r#"
                INSERT INTO batches (id, intent_data, created_at, state, participant_count)
                VALUES (?, ?, ?, ?, ?)
                "#
            )
            .bind(&batch_id)
            .bind(&intent_data)
            .bind(now)
            .bind("filling")
            .bind(0)
            .execute(&state.db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            Ok(Json(CreateBatchResponse {
                id: batch_id,
                message,
            }))
        }
        Err(e) => {
            // If already running, that's ok - just create a new batch record
            if e.contains("already running") {
                let batch_id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp();

                // Fetch coordinator config for intent data
                #[derive(sqlx::FromRow)]
                struct ConfigRow {
                    network: String,
                    fee_rate: i64,
                }

                let config: ConfigRow = sqlx::query_as(
                    "SELECT network, fee_rate FROM coordinator_config WHERE id = 1"
                )
                .fetch_one(&state.db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                // Create intent data JSON
                let intent_data = serde_json::json!({
                    "network": config.network,
                    "fee_rate": config.fee_rate,
                    "min_participants": payload.min_participants,
                    "max_participants": payload.max_participants,
                    "deadline_ms": payload.timeout_seconds * 1000,
                }).to_string();

                // Insert batch record
                sqlx::query(
                    r#"
                    INSERT INTO batches (id, intent_data, created_at, state, participant_count)
                    VALUES (?, ?, ?, ?, ?)
                    "#
                )
                .bind(&batch_id)
                .bind(&intent_data)
                .bind(now)
                .bind("filling")
                .bind(0)
                .execute(&state.db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                Ok(Json(CreateBatchResponse {
                    id: batch_id,
                    message: "Coordinator is already running and accepting participants".to_string(),
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
