use axum::{
    extract::{ws::WebSocket, State, WebSocketUpgrade},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use crate::{events::CoordinatorEvent, state::AppState};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Subscribe { channels: Vec<String> },
    Unsubscribe { channels: Vec<String> },
    Ping,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Event(CoordinatorEvent),
    Pong,
    Error { message: String },
}

/// WebSocket handler - upgrades HTTP connection to WebSocket
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = state.event_bus.subscribe();

    info!("🔌 WebSocket client connected");

    // Spawn a task to forward coordinator events to the WebSocket client
    let mut send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let message = ServerMessage::Event(event);
            let json = match serde_json::to_string(&message) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize event: {}", e);
                    continue;
                }
            };

            if sender
                .send(axum::extract::ws::Message::Text(json))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Handle incoming messages from the client
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                axum::extract::ws::Message::Text(text) => {
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Ping) => {
                            info!("📡 Received ping from client");
                        }
                        Ok(ClientMessage::Subscribe { channels }) => {
                            info!("📡 Client subscribed to channels: {:?}", channels);
                        }
                        Ok(ClientMessage::Unsubscribe { channels }) => {
                            info!("📡 Client unsubscribed from channels: {:?}", channels);
                        }
                        Err(e) => {
                            warn!("Failed to parse client message: {}", e);
                        }
                    }
                }
                axum::extract::ws::Message::Close(_) => {
                    info!("🔌 WebSocket client disconnected");
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }

    info!("🔌 WebSocket connection closed");
}
