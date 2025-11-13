use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use crate::events::CoordinatorEvent;
use crate::coordinator_manager::CoordinatorManager;

/// Application state shared across all API handlers
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool
    pub db: SqlitePool,

    /// Event bus for coordinator events (broadcast channel)
    pub event_bus: broadcast::Sender<CoordinatorEvent>,

    /// Coordinator manager for controlling the batch coordinator
    pub coordinator: Arc<RwLock<CoordinatorManager>>,
}

impl AppState {
    /// Broadcast an event to all subscribers
    pub fn broadcast_event(&self, event: CoordinatorEvent) {
        // Ignore error if no receivers (that's ok)
        let _ = self.event_bus.send(event);
    }
}
