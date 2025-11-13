use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use crate::{db::queries, db::schema::Batch, state::AppState};

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    100
}

#[derive(Serialize)]
pub struct HistoryResponse {
    pub batches: Vec<Batch>,
    pub total: usize,
}

/// Get batch history (completed and failed batches)
pub async fn get_history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, StatusCode> {
    // Get completed batches
    let mut batches = queries::get_batches(&state.db, Some("completed"))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Also get failed batches
    let failed_batches = queries::get_batches(&state.db, Some("failed"))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    batches.extend(failed_batches);

    // Sort by completion time (most recent first)
    batches.sort_by(|a, b| {
        b.completed_at
            .unwrap_or(0)
            .cmp(&a.completed_at.unwrap_or(0))
    });

    // Apply limit
    batches.truncate(query.limit as usize);

    let total = batches.len();

    Ok(Json(HistoryResponse { batches, total }))
}
