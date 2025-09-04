use automatafl_api::*;
use gloo_net::http::{Request, RequestBuilder};
use serde::de::DeserializeOwned;
use uuid::Uuid;

pub struct ApiClient {
    base_url: String,
    auth_token: Option<String>,
}

impl ApiClient {
    pub fn new(base_url: String, auth_token: Option<String>) -> Self {
        Self { base_url, auth_token }
    }

    fn request(&self, path: &str) -> RequestBuilder {
        let mut req = Request::get(&format!("{}{}", self.base_url, path));
        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", &format!("Bearer {}", token));
        }
        req
    }

    fn post_request(&self, path: &str) -> RequestBuilder {
        let mut req = Request::post(&format!("{}{}", self.base_url, path))
            .header("Content-Type", "application/json");
        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", &format!("Bearer {}", token));
        }
        req
    }

    async fn handle_response<T: DeserializeOwned>(req: RequestBuilder) -> Result<T, String> {
        let response = req.send().await.map_err(|e| e.to_string())?;
        
        if response.ok() {
            response.json::<T>().await.map_err(|e| e.to_string())
        } else {
            let error = response.json::<ErrorResponse>().await
                .map(|e| e.error)
                .unwrap_or_else(|_| format!("HTTP {}", response.status()));
            Err(error)
        }
    }

    // Auth endpoints
    pub async fn register(&self, username: String, password: String) -> Result<AuthResponse, String> {
        let req = self.post_request("/auth/register")
            .json(&RegisterRequest { username, password })
            .map_err(|e| e.to_string())?;
        Self::handle_response(req).await
    }

    pub async fn login(&self, username: String, password: String) -> Result<AuthResponse, String> {
        let req = self.post_request("/auth/login")
            .json(&LoginRequest { username, password })
            .map_err(|e| e.to_string())?;
        Self::handle_response(req).await
    }

    pub async fn get_me(&self) -> Result<UserInfo, String> {
        let req = self.request("/auth/me");
        Self::handle_response(req).await
    }

    // Game endpoints
    pub async fn list_games(&self) -> Result<Vec<GameStateResponse>, String> {
        let req = self.request("/games");
        Self::handle_response(req).await
    }

    pub async fn create_game(&self, time_control: Option<String>) -> Result<GameStateResponse, String> {
        let req = self.post_request("/games")
            .json(&CreateGameRequest { time_control })
            .map_err(|e| e.to_string())?;
        Self::handle_response(req).await
    }

    pub async fn get_game(&self, game_id: Uuid) -> Result<GameStateResponse, String> {
        let req = self.request(&format!("/games/{}", game_id));
        Self::handle_response(req).await
    }

    pub async fn join_game(&self, game_id: Uuid) -> Result<GameStateResponse, String> {
        let req = self.post_request(&format!("/games/{}/join", game_id));
        Self::handle_response(req).await
    }

    pub async fn submit_move(
        &self, 
        game_id: Uuid, 
        from_x: u8, 
        from_y: u8, 
        to_x: u8, 
        to_y: u8
    ) -> Result<SubmitMoveResponse, String> {
        let req = self.post_request(&format!("/games/{}/move", game_id))
            .json(&SubmitMoveRequest { from_x, from_y, to_x, to_y })
            .map_err(|e| e.to_string())?;
        Self::handle_response(req).await
    }

    pub async fn spectate_game(&self, game_id: Uuid) -> Result<GameStateResponse, String> {
        let req = self.request(&format!("/games/{}/spectate", game_id));
        Self::handle_response(req).await
    }

    // Chat endpoints
    pub async fn get_messages(&self, game_id: Uuid) -> Result<Vec<ChatMessage>, String> {
        let req = self.request(&format!("/games/{}/chat", game_id));
        Self::handle_response(req).await
    }

    pub async fn send_message(&self, game_id: Uuid, message: String) -> Result<ChatMessage, String> {
        let req = self.post_request(&format!("/games/{}/chat", game_id))
            .json(&SendChatRequest { message })
            .map_err(|e| e.to_string())?;
        Self::handle_response(req).await
    }

    // History endpoints
    pub async fn get_game_history(&self, game_id: Uuid) -> Result<GameHistoryResponse, String> {
        let req = self.request(&format!("/games/{}/history", game_id));
        Self::handle_response(req).await
    }

    pub async fn get_user_games(&self, user_id: Uuid, page: i32) -> Result<UserGamesResponse, String> {
        let req = self.request(&format!("/users/{}/games?page={}", user_id, page));
        Self::handle_response(req).await
    }

    // Leaderboard endpoint
    pub async fn get_leaderboard(&self, page: i32) -> Result<LeaderboardResponse, String> {
        let req = self.request(&format!("/leaderboard?page={}", page));
        Self::handle_response(req).await
    }

    // User profile endpoint
    pub async fn get_user_profile(&self, user_id: Uuid) -> Result<UserProfileResponse, String> {
        let req = self.request(&format!("/users/{}/profile", user_id));
        Self::handle_response(req).await
    }

    // Matchmaking endpoints
    pub async fn join_matchmaking(&self, request: JoinMatchmakingRequest) -> Result<(), String> {
        let req = self.post_request("/matchmaking/join")
            .json(&request)
            .map_err(|e| e.to_string())?;
        Self::handle_response::<serde_json::Value>(req).await.map(|_| ())
    }

    pub async fn leave_matchmaking(&self) -> Result<(), String> {
        let req = self.post_request("/matchmaking/leave");
        Self::handle_response::<serde_json::Value>(req).await.map(|_| ())
    }

    pub async fn matchmaking_status(&self) -> Result<MatchmakingStatusResponse, String> {
        let req = self.request("/matchmaking/status");
        Self::handle_response(req).await
    }

    // Health check endpoint
    pub async fn health_check(&self) -> Result<HealthCheckResponse, String> {
        let req = self.request("/health");
        Self::handle_response(req).await
    }

    // Metrics endpoint (returns raw text)
    pub async fn get_metrics(&self) -> Result<String, String> {
        let response = self.request("/metrics")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        
        if response.ok() {
            response.text().await.map_err(|e| e.to_string())
        } else {
            Err(format!("HTTP {}", response.status()))
        }
    }
}
