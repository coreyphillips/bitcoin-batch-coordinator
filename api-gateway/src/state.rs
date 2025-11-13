use sqlx::SqlitePool;
use tokio::sync::broadcast;
use crate::events::CoordinatorEvent;

/// Application state shared across all API handlers
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool
    pub db: SqlitePool,

    /// Event bus for coordinator events (broadcast channel)
    pub event_bus: broadcast::Sender<CoordinatorEvent>,
}

impl AppState {
    /// Broadcast an event to all subscribers
    pub fn broadcast_event(&self, event: CoordinatorEvent) {
        // Ignore error if no receivers (that's ok)
        let _ = self.event_bus.send(event);
    }
}
