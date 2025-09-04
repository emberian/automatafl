use automatafl_api::WebSocketMessage;
use futures::{StreamExt, SinkExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::prelude::*;
use std::sync::Arc;
use futures::lock::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct WebSocketConnection {
    tx: Arc<Mutex<Option<futures_channel::mpsc::UnboundedSender<WebSocketMessage>>>>,
}

impl WebSocketConnection {
    pub fn new() -> (Self, ReadSignal<Option<WebSocketMessage>>) {
        let (msg_signal, set_msg_signal) = create_signal(None);
        let connection = Self {
            tx: Arc::new(Mutex::new(None)),
        };
        (connection, msg_signal)
    }

    pub async fn connect(&self, auth_token: Option<String>, set_msg_signal: WriteSignal<Option<WebSocketMessage>>) {
        let ws_url = if cfg!(debug_assertions) {
            "ws://localhost:3000/api/ws"
        } else {
            let window = web_sys::window().unwrap();
            let location = window.location();
            let protocol = if location.protocol().unwrap() == "https:" { "wss:" } else { "ws:" };
            let host = location.host().unwrap();
            format!("{}//{}/api/ws", protocol, host)
        };

        let ws = match WebSocket::open(&ws_url) {
            Ok(ws) => ws,
            Err(e) => {
                leptos::logging::error!("Failed to connect to WebSocket: {:?}", e);
                return;
            }
        };

        let (mut write, mut read) = ws.split();
        let (tx, mut rx) = futures_channel::mpsc::unbounded();
        
        // Update our sender
        *self.tx.lock().await = Some(tx);

        // Send auth token if available
        if let Some(token) = auth_token {
            let auth_msg = serde_json::json!({
                "type": "auth",
                "token": token
            });
            if let Ok(msg) = serde_json::to_string(&auth_msg) {
                let _ = write.send(Message::Text(msg)).await;
            }
        }

        // Spawn tasks for reading and writing
        spawn_local(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<WebSocketMessage>(&text) {
                            Ok(ws_msg) => {
                                set_msg_signal.set(Some(ws_msg));
                            }
                            Err(e) => {
                                leptos::logging::error!("Failed to parse WebSocket message: {:?}", e);
                            }
                        }
                    }
                    Ok(Message::Bytes(_)) => {
                        leptos::logging::warn!("Received binary WebSocket message, ignoring");
                    }
                    Err(e) => {
                        leptos::logging::error!("WebSocket read error: {:?}", e);
                        break;
                    }
                }
            }
        });

        spawn_local(async move {
            while let Some(msg) = rx.next().await {
                match serde_json::to_string(&msg) {
                    Ok(text) => {
                        if let Err(e) = write.send(Message::Text(text)).await {
                            leptos::logging::error!("Failed to send WebSocket message: {:?}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        leptos::logging::error!("Failed to serialize WebSocket message: {:?}", e);
                    }
                }
            }
        });

        // Spawn ping task
        let tx_clone = self.tx.clone();
        spawn_local(async move {
            let mut interval = gloo_timers::future::IntervalStream::new(30_000);
            while interval.next().await.is_some() {
                if let Some(ref tx) = *tx_clone.lock().await {
                    let _ = tx.unbounded_send(WebSocketMessage::Ping);
                }
            }
        });
    }

    pub async fn send(&self, msg: WebSocketMessage) -> Result<(), String> {
        if let Some(ref tx) = *self.tx.lock().await {
            tx.unbounded_send(msg)
                .map_err(|_| "WebSocket connection closed".to_string())
        } else {
            Err("WebSocket not connected".to_string())
        }
    }

    pub async fn subscribe_to_game(&self, game_id: Uuid) -> Result<(), String> {
        self.send(WebSocketMessage::Subscribe { game_id }).await
    }

    pub async fn unsubscribe_from_game(&self, game_id: Uuid) -> Result<(), String> {
        self.send(WebSocketMessage::Unsubscribe { game_id }).await
    }

    pub async fn submit_move_ws(&self, game_id: Uuid, move_data: automatafl_api::SubmitMoveRequest) -> Result<(), String> {
        self.send(WebSocketMessage::SubmitMove { game_id, move_data }).await
    }

    pub async fn send_chat_ws(&self, game_id: Uuid, message: String) -> Result<(), String> {
        self.send(WebSocketMessage::SendChat { game_id, message }).await
    }
}