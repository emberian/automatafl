// WebSocket handler with auto-reconnection for the new backend protocol
// The backend sends GameEvent messages over WebSocket

#![allow(dead_code)] // Some methods are part of the public API but not yet used

use automatafl_api_types::GameEvent;
use futures::{SinkExt, StreamExt};
use futures_util::stream::SplitSink;
use gloo_net::websocket::{Message, futures::WebSocket};
use leptos::task::spawn_local;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};
use uuid::Uuid;

use crate::state::AppState;

pub struct WebSocketConnection {
    sink: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
    game_id: Uuid,
    ws_url: String,
    app_state: AppState,
    reconnect_attempts: Rc<RefCell<u32>>,
    is_dropped: Arc<AtomicBool>,
}

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY_MS: u32 = 2000;
const MAX_RECONNECT_DELAY_MS: u32 = 30000;

/// Helper to process WebSocket message stream
async fn handle_message_stream(
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    sink_ref: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
    game_id: Uuid,
    app_state: AppState,
) {
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // WebSocket protocol handles ping/pong frames automatically
                match serde_json::from_str::<GameEvent>(&text) {
                    Ok(event) => {
                        app_state.handle_game_event(game_id, event);
                    }
                    Err(e) => {
                        web_sys::console::error_1(
                            &format!("⚠️ Failed to parse GameEvent: {} - {}", e, text).into(),
                        );
                    }
                }
            }
            Ok(Message::Bytes(_)) => {
                web_sys::console::warn_1(&"⚠️ Received binary WebSocket message (ignored)".into());
            }
            Err(e) => {
                web_sys::console::error_1(&format!("❌ WebSocket error: {:?}", e).into());
                app_state.set_websocket_connected(game_id, false);
                break;
            }
        }
    }
}

impl WebSocketConnection {
    /// Private helper to setup connection and start message handler
    fn start_connection_and_handler(
        ws: WebSocket,
        game_id: Uuid,
        app_state: AppState,
        sink_ref: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
        reconnect_attempts: Rc<RefCell<u32>>,
        is_dropped: Arc<AtomicBool>,
        ws_url: String,
    ) {
        let (sink, stream) = ws.split();
        *sink_ref.borrow_mut() = Some(sink);
        app_state.set_websocket_connected(game_id, true);
        *reconnect_attempts.borrow_mut() = 0; // Reset on any successful connection

        let sink_ref_clone = sink_ref.clone();
        spawn_local(async move {
            handle_message_stream(stream, sink_ref_clone, game_id, app_state.clone()).await;

            // Connection closed
            app_state.set_websocket_connected(game_id, false);
            *sink_ref.borrow_mut() = None;
            web_sys::console::log_1(&format!("🔌 WebSocket closed for game {}", game_id).into());

            // Check if we should abort reconnection
            if is_dropped.load(std::sync::atomic::Ordering::Relaxed) {
                web_sys::console::log_1(
                    &format!(
                        "🔌 Aborting reconnect for game {} (connection dropped)",
                        game_id
                    )
                    .into(),
                );
                return;
            }

            // Schedule reconnection
            Self::schedule_reconnect(
                ws_url,
                game_id,
                app_state,
                sink_ref,
                reconnect_attempts,
                is_dropped,
            );
        });
    }
    /// Create a new WebSocket connection for a specific game with auto-reconnection
    pub fn new(game_id: Uuid, app_state: AppState) -> Self {
        let ws_url = format!("{}/api/v1/games/{}/ws", app_state.ws_base_url, game_id);

        let connection = Self {
            sink: Rc::new(RefCell::new(None)),
            game_id,
            ws_url: ws_url.clone(),
            app_state: app_state.clone(),
            reconnect_attempts: Rc::new(RefCell::new(0)),
            is_dropped: Arc::new(AtomicBool::new(false)),
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
        let is_dropped = self.is_dropped.clone();
        let ws_url_clone = ws_url.clone();

        web_sys::console::log_1(&format!("🔌 Connecting to WebSocket: {}", ws_url).into());

        spawn_local(async move {
            let ws = match WebSocket::open(&ws_url) {
                Ok(ws) => {
                    web_sys::console::log_1(&"✅ WebSocket opened successfully".into());
                    ws
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("❌ Failed to open WebSocket: {:?}", e).into(),
                    );
                    app_state.set_websocket_connected(game_id, false);

                    // Check if we should abort
                    if is_dropped.load(std::sync::atomic::Ordering::Relaxed) {
                        web_sys::console::log_1(
                            &format!(
                                "🔌 Aborting initial connect for game {} (connection dropped)",
                                game_id
                            )
                            .into(),
                        );
                        return;
                    }

                    // Schedule reconnection
                    Self::schedule_reconnect(
                        ws_url_clone,
                        game_id,
                        app_state,
                        sink_ref,
                        reconnect_attempts,
                        is_dropped,
                    );
                    return;
                }
            };

            Self::start_connection_and_handler(
                ws,
                game_id,
                app_state,
                sink_ref,
                reconnect_attempts,
                is_dropped,
                ws_url_clone,
            );
        });
    }

    fn schedule_reconnect(
        ws_url: String,
        game_id: Uuid,
        app_state: AppState,
        sink_ref: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
        reconnect_attempts: Rc<RefCell<u32>>,
        is_dropped: Arc<AtomicBool>,
    ) {
        spawn_local(async move {
            // Check if we should abort before even scheduling
            if is_dropped.load(std::sync::atomic::Ordering::Relaxed) {
                web_sys::console::log_1(
                    &format!(
                        "🔌 Aborting reconnect for game {} (connection dropped)",
                        game_id
                    )
                    .into(),
                );
                return;
            }

            let attempts = *reconnect_attempts.borrow();

            if attempts >= MAX_RECONNECT_ATTEMPTS {
                web_sys::console::error_1(
                    &format!(
                        "❌ Max reconnection attempts ({}) reached for game {}",
                        MAX_RECONNECT_ATTEMPTS, game_id
                    )
                    .into(),
                );
                return;
            }

            // Exponential backoff: 2s, 4s, 8s, 16s, 30s
            let delay = (RECONNECT_DELAY_MS * (1 << attempts)).min(MAX_RECONNECT_DELAY_MS);

            web_sys::console::log_1(
                &format!(
                    "🔄 Scheduling reconnection attempt {} in {}ms",
                    attempts + 1,
                    delay
                )
                .into(),
            );

            *reconnect_attempts.borrow_mut() += 1;

            // Wait before reconnecting
            gloo_timers::future::TimeoutFuture::new(delay).await;

            // Check again after the delay
            if is_dropped.load(std::sync::atomic::Ordering::Relaxed) {
                web_sys::console::log_1(
                    &format!(
                        "🔌 Aborting reconnect for game {} after delay (connection dropped)",
                        game_id
                    )
                    .into(),
                );
                return;
            }

            web_sys::console::log_1(
                &format!(
                    "🔄 Attempting reconnection {} of {}",
                    attempts, MAX_RECONNECT_ATTEMPTS
                )
                .into(),
            );

            // Try to reconnect
            let ws = match WebSocket::open(&ws_url) {
                Ok(ws) => {
                    web_sys::console::log_1(&"✅ Reconnected successfully!".into());
                    ws
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("❌ Reconnection failed: {:?}", e).into());
                    app_state.set_websocket_connected(game_id, false);

                    // Schedule another attempt
                    Self::schedule_reconnect(
                        ws_url,
                        game_id,
                        app_state,
                        sink_ref,
                        reconnect_attempts,
                        is_dropped,
                    );
                    return;
                }
            };

            Self::start_connection_and_handler(
                ws,
                game_id,
                app_state,
                sink_ref,
                reconnect_attempts,
                is_dropped,
                ws_url,
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

impl Drop for WebSocketConnection {
    fn drop(&mut self) {
        self.is_dropped
            .store(true, std::sync::atomic::Ordering::Relaxed);
        web_sys::console::log_1(
            &format!(
                "🔌 WebSocketConnection for game {} dropped. Cleanup signaled.",
                self.game_id
            )
            .into(),
        );
    }
}

/// Helper function to create WebSocket connection for a game
pub fn create_game_websocket(game_id: Uuid, app_state: &AppState) -> WebSocketConnection {
    WebSocketConnection::new(game_id, app_state.clone())
}
