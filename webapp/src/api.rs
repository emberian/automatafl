use automatafl_api::*;
use gloo_net::http::{Request, RequestBuilder};
use leptos::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    auth_token: Arc<RwSignal<Option<String>>>,
}

impl ApiClient {
    pub fn new(auth_token: Arc<RwSignal<Option<String>>>) -> Self {
        let base_url = if cfg!(debug_assertions) {
            "http://localhost:3000".to_string()
        } else {
            // In production, use the same origin
            "".to_string()
        };
        
        Self { base_url, auth_token }
    }

    fn request(&self, method: &str, path: &str) -> RequestBuilder {
        let mut req = Request::new(&format!("{}{}", self.base_url, path))
            .method(method);
        
        if let Some(token) = self.auth_token.get() {
            req = req.header("Authorization", &format!("Bearer {}", token));
        }
        
        req.header("Content-Type", "application/json")
    }

    async fn send_json<T: Serialize, R: DeserializeOwned>(&self, req: RequestBuilder, body: Option<&T>) -> Result<R, String> {
        let req = if let Some(body) = body {
            req.json(body).map_err(|e| e.to_string())?
        } else {
            req
        };
        
        let resp = req.send().await.map_err(|e| e.to_string())?;
        
        if !resp.ok() {
            let error: ErrorResponse = resp.json().await.unwrap_or_else(|_| ErrorResponse {
                error: "Unknown error".to_string(),
            });
            return Err(error.error);
        }
        
        resp.json().await.map_err(|e| e.to_string())
    }

    // Auth endpoints
    pub async fn register(&self, username: String, password: String) -> Result<AuthResponse, String> {
        let req = RegisterRequest { username, password };
        self.send_json(self.request("POST", "/api/auth/register"), Some(&req)).await
    }

    pub async fn login(&self, username: String, password: String) -> Result<AuthResponse, String> {
        let req = LoginRequest { username, password };
        self.send_json(self.request("POST", "/api/auth/login"), Some(&req)).await
    }

    pub async fn get_me(&self) -> Result<UserInfo, String> {
        self.send_json::<(), UserInfo>(self.request("GET", "/api/auth/me"), None).await
    }

    // Game endpoints
    pub async fn list_games(&self) -> Result<Vec<GameStateResponse>, String> {
        self.send_json::<(), Vec<GameStateResponse>>(self.request("GET", "/api/games"), None).await
    }

    pub async fn create_game(&self, time_control: Option<String>) -> Result<GameStateResponse, String> {
        let req = CreateGameRequest { time_control };
        self.send_json(self.request("POST", "/api/games"), Some(&req)).await
    }

    pub async fn get_game(&self, game_id: Uuid) -> Result<GameStateResponse, String> {
        self.send_json::<(), GameStateResponse>(
            self.request("GET", &format!("/api/games/{}", game_id)),
            None
        ).await
    }

    pub async fn join_game(&self, game_id: Uuid) -> Result<GameStateResponse, String> {
        self.send_json::<(), GameStateResponse>(
            self.request("POST", &format!("/api/games/{}/join", game_id)),
            None
        ).await
    }

    pub async fn submit_move(&self, game_id: Uuid, move_data: SubmitMoveRequest) -> Result<SubmitMoveResponse, String> {
        self.send_json(
            self.request("POST", &format!("/api/games/{}/move", game_id)),
            Some(&move_data)
        ).await
    }

    pub async fn spectate_game(&self, game_id: Uuid) -> Result<GameStateResponse, String> {
        self.send_json::<(), GameStateResponse>(
            self.request("GET", &format!("/api/games/{}/spectate", game_id)),
            None
        ).await
    }

    // Chat endpoints
    pub async fn get_messages(&self, game_id: Uuid) -> Result<Vec<ChatMessage>, String> {
        self.send_json::<(), Vec<ChatMessage>>(
            self.request("GET", &format!("/api/games/{}/chat", game_id)),
            None
        ).await
    }

    pub async fn send_message(&self, game_id: Uuid, message: String) -> Result<ChatMessage, String> {
        let req = SendChatRequest { message };
        self.send_json(
            self.request("POST", &format!("/api/games/{}/chat", game_id)),
            Some(&req)
        ).await
    }

    // History endpoints
    pub async fn get_game_history(&self, game_id: Uuid) -> Result<GameHistoryResponse, String> {
        self.send_json::<(), GameHistoryResponse>(
            self.request("GET", &format!("/api/games/{}/history", game_id)),
            None
        ).await
    }

    pub async fn get_user_games(&self, user_id: Uuid, page: Option<i32>) -> Result<UserGamesResponse, String> {
        let url = if let Some(page) = page {
            format!("/api/users/{}/games?page={}", user_id, page)
        } else {
            format!("/api/users/{}/games", user_id)
        };
        self.send_json::<(), UserGamesResponse>(self.request("GET", &url), None).await
    }

    // Leaderboard endpoint
    pub async fn get_leaderboard(&self, page: Option<i32>) -> Result<LeaderboardResponse, String> {
        let url = if let Some(page) = page {
            format!("/api/leaderboard?page={}", page)
        } else {
            "/api/leaderboard".to_string()
        };
        self.send_json::<(), LeaderboardResponse>(self.request("GET", &url), None).await
    }

    // User profile endpoint
    pub async fn get_user_profile(&self, user_id: Uuid) -> Result<UserProfileResponse, String> {
        self.send_json::<(), UserProfileResponse>(
            self.request("GET", &format!("/api/users/{}/profile", user_id)),
            None
        ).await
    }

    // Matchmaking endpoints
    pub async fn join_matchmaking(&self, time_control: Option<String>, rating_range: Option<(i32, i32)>) -> Result<(), String> {
        let req = JoinMatchmakingRequest { time_control, rating_range };
        self.send_json::<JoinMatchmakingRequest, serde_json::Value>(
            self.request("POST", "/api/matchmaking/join"),
            Some(&req)
        ).await.map(|_| ())
    }

    pub async fn leave_matchmaking(&self) -> Result<(), String> {
        self.send_json::<(), serde_json::Value>(
            self.request("POST", "/api/matchmaking/leave"),
            None
        ).await.map(|_| ())
    }

    pub async fn matchmaking_status(&self) -> Result<MatchmakingStatusResponse, String> {
        self.send_json::<(), MatchmakingStatusResponse>(
            self.request("GET", "/api/matchmaking/status"),
            None
        ).await
    }

    // Health check
    pub async fn health_check(&self) -> Result<HealthCheckResponse, String> {
        self.send_json::<(), HealthCheckResponse>(
            self.request("GET", "/api/health"),
            None
        ).await
    }
}
