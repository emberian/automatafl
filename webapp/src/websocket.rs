// WebSocket handler for the new backend protocol
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
}

impl WebSocketConnection {
    /// Create a new WebSocket connection for a specific game
    pub fn new(game_id: Uuid, app_state: AppState) -> Self {
        let ws_url = format!("{}/api/v1/games/{}/ws", app_state.ws_base_url, game_id);
        
        web_sys::console::log_1(&format!("Connecting to WebSocket: {}", ws_url).into());
        
        let ws = match WebSocket::open(&ws_url) {
            Ok(ws) => ws,
            Err(e) => {
                web_sys::console::error_1(&format!("Failed to open WebSocket: {:?}", e).into());
                // Return a disconnected connection
                return Self {
                    sink: Rc::new(RefCell::new(None)),
                    game_id,
                };
            }
        };
        
        let (sink, mut stream) = ws.split();
        let sink = Rc::new(RefCell::new(Some(sink)));
        
        let connection = Self {
            sink: sink.clone(),
            game_id,
        };

        // Handle incoming messages
        let game_id_clone = game_id;
        spawn_local(async move {
            app_state.websocket_connected.set(true);
            
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<GameEvent>(&text) {
                            Ok(event) => {
                                app_state.handle_game_event(game_id_clone, event);
                            }
                            Err(e) => {
                                web_sys::console::error_1(
                                    &format!("Failed to parse GameEvent: {} - {}", e, text).into()
                                );
                            }
                        }
                    }
                    Ok(Message::Bytes(_)) => {
                        web_sys::console::warn_1(&"Received binary WebSocket message (ignored)".into());
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("WebSocket error: {:?}", e).into());
                        app_state.websocket_connected.set(false);
                        break;
                    }
                }
            }
            
            // Connection closed
            app_state.websocket_connected.set(false);
            *sink.borrow_mut() = None;
            web_sys::console::log_1(&format!("WebSocket closed for game {}", game_id_clone).into());
        });

        connection
    }

    /// Send a ping to keep the connection alive
    /// Note: The backend sends pings automatically, but we can respond to them
    pub async fn send_ping(&self) -> Result<(), String> {
        if let Some(sink) = self.sink.borrow_mut().as_mut() {
            sink.send(Message::Text("ping".to_string()))
                .await
                .map_err(|e| format!("Failed to send ping: {:?}", e))
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

