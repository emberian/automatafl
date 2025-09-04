use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use axum_extra::TypedHeader;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use uuid::Uuid;

use automatafl_api::{WebSocketMessage, GameStateResponse, ChatMessage as ApiChatMessage};

use crate::{
    auth::{AuthUser, Claims},
    error::{AppError, Result},
    state::AppState,
};

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    TypedHeader(auth_header): TypedHeader<headers::Authorization<headers::authorization::Bearer>>,
) -> Result<Response> {
    // Verify JWT token
    let token = auth_header.token();
    let secret = state.jwt_secret.clone();
    
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|_| AppError::Auth("Invalid token".to_string()))?;
    
    let auth_user = AuthUser {
        id: token_data.claims.sub,
        username: token_data.claims.username,
    };
    
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, auth_user)))
}

async fn handle_socket(
    socket: axum::extract::ws::WebSocket,
    state: AppState,
    auth_user: AuthUser,
) {
    // Record connection metric
    crate::metrics::record_websocket_connection("connect");
    
    let (mut sender, mut receiver) = socket.split();
    
    // Channel for sending messages to this client
    let (tx, mut rx) = tokio::sync::mpsc::channel::<WebSocketMessage>(100);
    
    // Spawn task to forward messages from channel to websocket
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // Main message handling loop
    let mut recv_task = {
        let tx = tx.clone();
        let state = state.clone();
        let user_id = auth_user.id;
        
        tokio::spawn(async move {
            while let Some(Ok(msg)) = receiver.next().await {
                if let Ok(text) = msg.to_text() {
                    if let Ok(ws_msg) = serde_json::from_str::<WebSocketMessage>(text) {
                        handle_websocket_message(ws_msg, &state, user_id, &tx).await;
                    }
                }
            }
        })
    };
    
    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }
    
    // Record disconnection metric
    crate::metrics::record_websocket_connection("disconnect");
}

async fn handle_websocket_message(
    msg: WebSocketMessage,
    state: &AppState,
    _user_id: Uuid,
    tx: &tokio::sync::mpsc::Sender<WebSocketMessage>,
) {
    match msg {
        WebSocketMessage::Subscribe { game_id } => {
            // Subscribe to game updates
            let sender = state.game_broadcasts
                .entry(game_id)
                .or_insert_with(|| broadcast::channel(100).0)
                .clone();
            
            let mut receiver = sender.subscribe();
            let tx = tx.clone();
            
            // Send confirmation
            let _ = tx.send(WebSocketMessage::Subscribed { game_id }).await;
            
            // Forward game updates to this client
            tokio::spawn(async move {
                while let Ok(update) = receiver.recv().await {
                    if let Ok(msg) = serde_json::from_str::<WebSocketMessage>(&update) {
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                }
            });
        }
        
        WebSocketMessage::Unsubscribe { game_id } => {
            // In a real implementation, we'd track subscriptions and clean them up
            let _ = tx.send(WebSocketMessage::Unsubscribed { game_id }).await;
        }
        
        WebSocketMessage::SubmitMove { game_id: _, move_data: _ } => {
            // Call the existing submit_move handler logic
            // For now, just send an error - in a real implementation,
            // we'd refactor the submit_move logic to be reusable here
            let _ = tx.send(WebSocketMessage::Error { 
                error: "Move submission via WebSocket not yet implemented".to_string() 
            }).await;
        }
        
        WebSocketMessage::SendChat { game_id: _, message: _ } => {
            // Similar to SubmitMove, we'd refactor the chat logic
            let _ = tx.send(WebSocketMessage::Error { 
                error: "Chat via WebSocket not yet implemented".to_string() 
            }).await;
        }
        
        WebSocketMessage::Ping => {
            let _ = tx.send(WebSocketMessage::Pong).await;
        }
        
        _ => {
            // Ignore server->client messages
        }
    }
}

// Helper function to broadcast game state updates
pub async fn broadcast_game_update(
    state: &AppState,
    game_id: Uuid,
    game_state: GameStateResponse,
) {
    if let Some(sender) = state.game_broadcasts.get(&game_id) {
        let msg = WebSocketMessage::GameUpdate { game_state };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = sender.send(json);
        }
    }
}

// Helper function to broadcast chat messages
pub async fn broadcast_chat_message(
    state: &AppState,
    game_id: Uuid,
    message: ApiChatMessage,
) {
    if let Some(sender) = state.game_broadcasts.get(&game_id) {
        let msg = WebSocketMessage::ChatMessage { message };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = sender.send(json);
        }
    }
}
