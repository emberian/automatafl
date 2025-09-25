use automatafl_api::{WebSocketMessage};
use futures::{SinkExt, StreamExt};
use futures_util::stream::SplitSink;
use gloo_net::websocket::{futures::WebSocket, Message, WebSocketError};
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use uuid::Uuid;
// wasm_bindgen_futures is already included in leptos prelude

use crate::state::AppState;

pub struct WebSocketConnection {
    sink: Rc<RefCell<Option<SplitSink<WebSocket, Message>>>>,
}

impl WebSocketConnection {
    pub fn new(url: String, app_state: AppState) -> Self {
        let ws = WebSocket::open(&url).expect("Failed to connect to WebSocket");
        let (sink, mut stream) = ws.split();
        
        let sink = Rc::new(RefCell::new(Some(sink)));
        let connection = Self {
            sink: sink.clone(),
        };

        // Handle incoming messages
        spawn_local(async move {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(&text) {
                            app_state.handle_websocket_message(ws_msg);
                        } else {
                            web_sys::console::error_1(&format!("Failed to parse WebSocket message: {}", text).into());
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
        });

        app_state.websocket_connected.set(true);
        
        // Start ping/pong keepalive
        let sink_clone = sink.clone();
        spawn_local(async move {
            let mut interval = gloo_timers::future::IntervalStream::new(30_000); // 30 seconds
            while let Some(_) = interval.next().await {
                if let Err(e) = connection.send_message(WebSocketMessage::Ping).await {
                    web_sys::console::error_1(&format!("Failed to send ping: {:?}", e).into());
                    break;
                }
            }
        });

        Self { sink: sink_clone }
    }

    pub async fn send_message(&self, msg: WebSocketMessage) -> Result<(), WebSocketError> {
        if let Some(sink) = self.sink.borrow_mut().as_mut() {
            let json = serde_json::to_string(&msg).map_err(|e| {
                WebSocketError::MessageSendError(gloo_utils::errors::JsError::new(e.to_string()))
            })?;
            sink.send(Message::Text(json)).await
        } else {
            Err(WebSocketError::ConnectionError(gloo_utils::errors::JsError::new("WebSocket not connected".to_string())))
        }
    }

    pub async fn subscribe_to_game(&self, game_id: Uuid) -> Result<(), WebSocketError> {
        self.send_message(WebSocketMessage::Subscribe { game_id }).await
    }

    pub async fn unsubscribe_from_game(&self, game_id: Uuid) -> Result<(), WebSocketError> {
        self.send_message(WebSocketMessage::Unsubscribe { game_id }).await
    }

    pub async fn submit_move(
        &self,
        game_id: Uuid,
        from_x: u8,
        from_y: u8,
        to_x: u8,
        to_y: u8,
    ) -> Result<(), WebSocketError> {
        self.send_message(WebSocketMessage::SubmitMove {
            game_id,
            move_data: automatafl_api::SubmitMoveRequest {
                from_x,
                from_y,
                to_x,
                to_y,
            },
        }).await
    }

    pub async fn send_chat(&self, game_id: Uuid, message: String) -> Result<(), WebSocketError> {
        self.send_message(WebSocketMessage::SendChat { game_id, message }).await
    }
}

// Helper function to create WebSocket connection with auth
pub fn create_websocket_connection(app_state: &AppState) -> Option<WebSocketConnection> {
    if let Some(token) = app_state.auth_token.get() {
        let url = format!("{}?token={}", app_state.ws_base_url, token);
        Some(WebSocketConnection::new(url, app_state.clone()))
    } else {
        None
    }
}
