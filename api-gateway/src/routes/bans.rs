use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::Serialize;
use crate::{db::queries, db::schema::Ban, state::AppState};

#[derive(Serialize)]
pub struct ListBansResponse {
    pub bans: Vec<Ban>,
    pub total: usize,
}

/// List all active bans
pub async fn list_bans(
    State(state): State<AppState>,
) -> Result<Json<ListBansResponse>, StatusCode> {
    let bans = queries::get_bans(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = bans.len();

    Ok(Json(ListBansResponse { bans, total }))
}
