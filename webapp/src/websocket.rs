// WebSocket handler with auto-reconnection for the new backend protocol
// The backend sends GameEvent messages over WebSocket

use automatafl_api_types::GameEvent;
use futures::{SinkExt, StreamExt};
use futures_util::stream::SplitSink;
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::cell::RefCell;
use std::rc::Rc;
use uuid::Uuid;

use crate::state::AppState;

pub struct WebSocketConnection {
    sink: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
    game_id: Uuid,
    ws_url: String,
    app_state: AppState,
    reconnect_attempts: Rc<RefCell<u32>>,
}

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY_MS: u32 = 2000;
const MAX_RECONNECT_DELAY_MS: u32 = 30000;

impl WebSocketConnection {
    /// Create a new WebSocket connection for a specific game with auto-reconnection
    pub fn new(game_id: Uuid, app_state: AppState) -> Self {
        let ws_url = format!("{}/api/v1/games/{}/ws", app_state.ws_base_url, game_id);
        
        let connection = Self {
            sink: Rc::new(RefCell::new(None)),
            game_id,
            ws_url: ws_url.clone(),
            app_state: app_state.clone(),
            reconnect_attempts: Rc::new(RefCell::new(0)),
        };
        
        connection.connect();
        connection
    }

    fn connect(&self) {
        let ws_url = self.ws_url.clone();
        let game_id = self.game_id;
        let app_state = self.app_state.clone();
        let sink_ref = self.sink.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();
        let ws_url_clone = ws_url.clone();
        let app_state_clone = app_state.clone();
        
        web_sys::console::log_1(&format!("🔌 Connecting to WebSocket: {}", ws_url).into());
        
        spawn_local(async move {
            let ws = match WebSocket::open(&ws_url) {
                Ok(ws) => {
                    web_sys::console::log_1(&"✅ WebSocket opened successfully".into());
                    *reconnect_attempts.borrow_mut() = 0; // Reset on successful connection
                    ws
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("❌ Failed to open WebSocket: {:?}", e).into());
                    app_state.websocket_connected.set(false);
                    
                    // Schedule reconnection
                    Self::schedule_reconnect(
                        ws_url_clone,
                        game_id,
                        app_state_clone,
                        sink_ref.clone(),
                        reconnect_attempts.clone(),
                    );
                    return;
                }
            };
            
            let (sink, mut stream) = ws.split();
            *sink_ref.borrow_mut() = Some(sink);
            app_state.websocket_connected.set(true);
            
            // Handle incoming messages
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // Handle ping/pong
                        if text == "ping" {
                            // Backend sends pings, we just acknowledge receipt
                            continue;
                        }
                        
                        match serde_json::from_str::<GameEvent>(&text) {
                            Ok(event) => {
                                app_state.handle_game_event(game_id, event);
                            }
                            Err(e) => {
                                web_sys::console::error_1(
                                    &format!("⚠️ Failed to parse GameEvent: {} - {}", e, text).into()
                                );
                            }
                        }
                    }
                    Ok(Message::Bytes(_)) => {
                        web_sys::console::warn_1(&"⚠️ Received binary WebSocket message (ignored)".into());
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("❌ WebSocket error: {:?}", e).into());
                        app_state.websocket_connected.set(false);
                        break;
                    }
                }
            }
            
            // Connection closed
            app_state.websocket_connected.set(false);
            *sink_ref.borrow_mut() = None;
            web_sys::console::log_1(&format!("🔌 WebSocket closed for game {}", game_id).into());
            
            // Schedule reconnection
            Self::schedule_reconnect(
                ws_url_clone,
                game_id,
                app_state_clone,
                sink_ref.clone(),
                reconnect_attempts.clone(),
            );
        });
    }

    fn schedule_reconnect(
        ws_url: String,
        game_id: Uuid,
        app_state: AppState,
        sink_ref: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
        reconnect_attempts: Rc<RefCell<u32>>,
    ) {
        spawn_local(async move {
            let attempts = *reconnect_attempts.borrow();
            
            if attempts >= MAX_RECONNECT_ATTEMPTS {
                web_sys::console::error_1(&format!(
                    "❌ Max reconnection attempts ({}) reached for game {}",
                    MAX_RECONNECT_ATTEMPTS, game_id
                ).into());
                return;
            }
            
            // Exponential backoff: 2s, 4s, 8s, 16s, 30s
            let delay = (RECONNECT_DELAY_MS * (1 << attempts)).min(MAX_RECONNECT_DELAY_MS);
            
            web_sys::console::log_1(&format!(
                "🔄 Scheduling reconnection attempt {} in {}ms",
                attempts + 1,
                delay
            ).into());
            
            *reconnect_attempts.borrow_mut() += 1;
            
            // Wait before reconnecting
            gloo_timers::future::TimeoutFuture::new(delay).await;
            
            web_sys::console::log_1(&format!(
                "🔄 Attempting reconnection {} of {}",
                attempts + 1,
                MAX_RECONNECT_ATTEMPTS
            ).into());
            
            // Try to reconnect
            let ws = match WebSocket::open(&ws_url) {
                Ok(ws) => {
                    web_sys::console::log_1(&"✅ Reconnected successfully!".into());
                    *reconnect_attempts.borrow_mut() = 0; // Reset counter
                    ws
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("❌ Reconnection failed: {:?}", e).into());
                    app_state.websocket_connected.set(false);
                    
                    // Schedule another attempt
                    Self::schedule_reconnect(
                        ws_url,
                        game_id,
                        app_state,
                        sink_ref,
                        reconnect_attempts,
                    );
                    return;
                }
            };
            
            let (sink, mut stream) = ws.split();
            *sink_ref.borrow_mut() = Some(sink);
            app_state.websocket_connected.set(true);
            
            // Continue handling messages (same as initial connection)
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if text == "ping" {
                            continue;
                        }
                        
                        match serde_json::from_str::<GameEvent>(&text) {
                            Ok(event) => {
                                app_state.handle_game_event(game_id, event);
                            }
                            Err(e) => {
                                web_sys::console::error_1(
                                    &format!("⚠️ Failed to parse GameEvent: {} - {}", e, text).into()
                                );
                            }
                        }
                    }
                    Ok(Message::Bytes(_)) => {
                        web_sys::console::warn_1(&"⚠️ Received binary message (ignored)".into());
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("❌ WebSocket error: {:?}", e).into());
                        app_state.websocket_connected.set(false);
                        break;
                    }
                }
            }
            
            // Connection closed again, schedule reconnect
            app_state.websocket_connected.set(false);
            *sink_ref.borrow_mut() = None;
            web_sys::console::log_1(&"🔌 WebSocket closed, will retry...".into());
            
            Self::schedule_reconnect(
                ws_url,
                game_id,
                app_state,
                sink_ref,
                reconnect_attempts,
            );
        });
    }

    /// Send a message (for future features like client-side events)
    pub async fn send_message(&self, msg: String) -> Result<(), String> {
        if let Some(sink) = self.sink.borrow_mut().as_mut() {
            sink.send(Message::Text(msg))
                .await
                .map_err(|e| format!("Failed to send message: {:?}", e))
        } else {
            Err("WebSocket not connected".to_string())
        }
    }

    pub fn is_connected(&self) -> bool {
        self.sink.borrow().is_some()
    }

    pub fn game_id(&self) -> Uuid {
        self.game_id
    }
}

/// Helper function to create WebSocket connection for a game
pub fn create_game_websocket(game_id: Uuid, app_state: &AppState) -> WebSocketConnection {
    WebSocketConnection::new(game_id, app_state.clone())
}
