mod api;
mod components;
mod helpers;
mod pages;
mod state;
mod utils;
mod websocket;

use components::{
    ConnectionStatus, KeyboardContext, KeyboardShortcutsHelp, ModalContainer, ModalContext,
    NetworkStatus, ToastContainer, ToastContext,
};
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use pages::*;
use state::AppState;

#[component]
fn LogoutButton(app_state: AppState) -> impl IntoView {
    let navigate = leptos_router::hooks::use_navigate();
    view! {
        <button
            class="button button-small"
            on:click=move |_| {
                app_state.logout();
                navigate("/", Default::default());
            }
        >
            "Logout"
        </button>
    }
}

#[component]
fn App() -> impl IntoView {
    let app_state = AppState::new();
    provide_context(app_state.clone());

    let toast_ctx = ToastContext::new();
    provide_context(toast_ctx);

    let modal_ctx = ModalContext::new();
    provide_context(modal_ctx);

    let kb_ctx = KeyboardContext::new();
    provide_context(kb_ctx);

    let app_state_for_nav = app_state.clone();
    let app_state_for_nav_leader = app_state.clone();
    let app_state_for_auth = app_state.clone();
    let app_state_for_profile = app_state.clone();
    let app_state_for_logout = app_state.clone();

    view! {
        <Router>
            <ToastContainer />
            <ModalContainer />
            <KeyboardShortcutsHelp />
            <NetworkStatus />
            <main>
                <nav class="main-nav">
                    <div class="nav-container">
                        <a href="/" class="nav-brand">"Automatafl"</a>
                        <div class="nav-links">
                            <a href="/games">"Games"</a>
                            <Show when=move || app_state_for_nav.is_authenticated.get()>
                                <a href="/games/create">"Create Game"</a>
                                <a href="/matchmaking">"Quick Match"</a>
                            </Show>
                            <a href="/leaderboard">"Leaderboard"</a>
                            <Show when=move || app_state_for_nav_leader.is_authenticated.get()>
                                {{
                                    move || {
                                        let player_id = app_state_for_profile.current_player_id.get();
                                        player_id.map(|id| view! {
                                            <a href=format!("/users/{}", id)>"Profile"</a>
                                        })
                                    }
                                }}
                            </Show>
                            <Show when=move || app_state_for_profile.is_admin()>
                                <a href="/admin">"Admin"</a>
                            </Show>
                            <a href="/health">"Status"</a>
                        </div>
                        <div class="nav-auth">
                            <ConnectionStatus />
                            <Show
                                when=move || app_state_for_auth.is_authenticated.get()
                                fallback=|| view! {
                                    <a href="/login" class="button button-small">"Login"</a>
                                    <a href="/register" class="button button-small button-primary">"Register"</a>
                                }
                            >
                                <LogoutButton app_state=app_state_for_logout.clone() />
                            </Show>
                        </div>
                    </div>
                </nav>

                <div class="main-content">
                    <Routes fallback=|| view! {
                        <div class="page-not-found">
                            <h1>"404 - Page Not Found"</h1>
                            <p>"The page you're looking for doesn't exist."</p>
                            <a href="/" class="button button-primary">"Go Home"</a>
                        </div>
                    }>
                        <Route path=path!("/") view=HomePage />
                        <Route path=path!("/login") view=LoginPage />
                        <Route path=path!("/register") view=RegisterPage />
                        <Route path=path!("/games") view=GamesListPage />
                        <Route path=path!("/games/create") view=CreateGamePage />
                        <Route path=path!("/games/:id") view=GamePage />

                        // Health & Status
                        <Route path=path!("/health") view=HealthDashboardPage />

                        // Stub pages for future features
                        <Route path=path!("/matchmaking") view=MatchmakingPage />
                        <Route path=path!("/leaderboard") view=LeaderboardPage />
                        <Route path=path!("/users/:id") view=UserProfilePage />
                        <Route path=path!("/users/:id/games") view=UserGamesPage />

                        // Game-related stubs
                        <Route path=path!("/games/:id/history") view=GameHistoryPage />
                        <Route path=path!("/games/:id/spectate") view=SpectatePage />

                        // Admin page
                        <Route path=path!("/admin") view=AdminPage />
                    </Routes>
                </div>

                <footer class="main-footer">
                    <div class="footer-content">
                        <p>"Automatafl - A strategic particle movement game"</p>
                        <p class="footer-links">
                            <a href="/health">"System Status"</a>
                            {" · "}
                            <a href="https://github.com/automatafl" target="_blank">"GitHub"</a>
                        </p>
                    </div>
                </footer>
            </main>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
