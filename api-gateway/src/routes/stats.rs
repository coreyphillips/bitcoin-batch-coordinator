use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use crate::{db::queries, state::AppState};

#[derive(Deserialize)]
pub struct StatsQuery {
    #[serde(default = "default_days")]
    days: i64,
}

fn default_days() -> i64 {
    30
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub daily_stats: Vec<crate::db::schema::Stats>,
    pub total_batches: i64,
    pub total_participants: i64,
    pub total_fees_saved: i64,
}

/// Get statistics and fee savings
pub async fn get_stats(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsResponse>, StatusCode> {
    let daily_stats = queries::get_stats(&state.db, query.days)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Calculate totals
    let total_batches: i64 = daily_stats.iter().map(|s| s.batches_completed).sum();
    let total_participants: i64 = daily_stats.iter().map(|s| s.total_participants).sum();
    let total_fees_saved: i64 = daily_stats.iter().map(|s| s.total_fees_saved).sum();

    Ok(Json(StatsResponse {
        daily_stats,
        total_batches,
        total_participants,
        total_fees_saved,
    }))
}
