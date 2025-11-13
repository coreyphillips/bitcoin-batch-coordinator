use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use clap::Parser;
use serde::Serialize;
use tokio::sync::broadcast;
use tower_http::{
    trace::TraceLayer,
    compression::CompressionLayer,
};
use tracing::{info, warn};

mod crypto;
mod db;
mod routes;
mod state;
mod events;

use state::AppState;
use events::CoordinatorEvent;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// API server host
    #[arg(long, env = "API_HOST", default_value = "0.0.0.0")]
    host: String,

    /// API server port
    #[arg(long, env = "API_PORT", default_value = "3000")]
    port: u16,

    /// Database URL
    #[arg(long, env = "DATABASE_URL", default_value = "sqlite:///data/coordinator.db")]
    database_url: String,

    /// Web UI static files directory
    #[arg(long, env = "WEB_DIR", default_value = "./web")]
    web_dir: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    timestamp: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
        )
        .init();

    // Parse command line arguments
    let args = Args::parse();

    info!("🚀 Starting Bitcoin Batch Coordinator API Gateway");
    info!("   Version: {}", env!("CARGO_PKG_VERSION"));
    info!("   API Server: {}:{}", args.host, args.port);
    info!("   Database: {}", args.database_url);

    // Initialize database
    info!("📦 Connecting to database...");
    let db_pool = db::init_database(&args.database_url).await?;
    info!("✅ Database connected and migrations applied");

    // Create event bus for coordinator events
    let (event_tx, _) = broadcast::channel::<CoordinatorEvent>(1000);

    // Create application state
    let app_state = AppState {
        db: db_pool,
        event_bus: event_tx,
    };

    // Build API routes
    let api_routes = Router::new()
        .route("/status", get(health_check))
        .route("/batches", get(routes::batches::list_batches))
        .route("/batches", post(routes::batches::create_batch))
        .route("/batches/:id", get(routes::batches::get_batch))
        .route("/batches/:id/participants", get(routes::batches::list_participants))
        .route("/history", get(routes::history::get_history))
        .route("/stats", get(routes::stats::get_stats))
        .route("/config", get(routes::config::get_config))
        .route("/config", post(routes::config::update_config))
        .route("/bans", get(routes::bans::list_bans))
        .route("/ws", get(routes::websocket::websocket_handler))
        .route("/identity/current", get(routes::identity::get_current_identity))
        .route("/identity/import-file", post(routes::identity::import_from_file))
        .route("/identity/import-phrase", post(routes::identity::import_from_phrase))
        .route("/identity", axum::routing::delete(routes::identity::delete_identity))
        .with_state(app_state);

    // Build main app with middleware
    let app = Router::new()
        .nest("/api/v1", api_routes)
        .fallback_service(
            tower_http::services::ServeDir::new(&args.web_dir)
                .fallback(tower_http::services::ServeFile::new(format!("{}/index.html", args.web_dir)))
        )
        .layer(
            tower_http::cors::CorsLayer::permissive()
                .allow_origin(tower_http::cors::Any)
        )
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = format!("{}:{}", args.host, args.port);
    info!("🌐 API Gateway listening on http://{}", addr);
    info!("📡 WebSocket endpoint: ws://{}/api/v1/ws", addr);
    info!("🎨 Dashboard: http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    // Check database connection
    let db_check = sqlx::query("SELECT 1").fetch_one(&state.db).await;

    if db_check.is_err() {
        warn!("Database health check failed");
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    Ok(Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    }))
}
