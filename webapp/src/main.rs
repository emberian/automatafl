mod api;
mod components;
mod pages;
mod state;
mod websocket;

use api::ApiClient;
use components::*;
use leptos::prelude::*;
use leptos_router::*;
use pages::*;
use state::{AppState, MatchmakingState};
use uuid::Uuid;

#[component]
fn App() -> impl IntoView {
    let app_state = AppState::new();
    provide_context(app_state.clone());

    view! {
        <Router>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path="/" view=HomePage />
                    <Route path="/login" view=LoginPage />
                    <Route path="/register" view=RegisterPage />
                    <Route path="/games" view=GamesListPage />
                    <Route path="/games/create" view=CreateGamePage />
                    <Route path="/games/:id" view=GamePage />
                    <Route path="/games/:id/history" view=GameHistoryPage />
                    <Route path="/games/:id/spectate" view=SpectatePage />
                    <Route path="/users/:id" view=UserProfilePage />
                    <Route path="/users/:id/games" view=UserGamesPage />
                    <Route path="/leaderboard" view=LeaderboardPage />
                    <Route path="/matchmaking" view=MatchmakingPage />
                    <Route path="/health" view=HealthDashboardPage />
                    <Route path="/metrics" view=MetricsPage />
                </Routes>
            </main>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App);
}