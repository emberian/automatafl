mod api;
mod components;
mod helpers;
mod pages;
mod state;
mod utils;
mod websocket;

use components::{
    ConnectionStatus, KeyboardContext, KeyboardListener, KeyboardShortcutsHelp, ModalContainer,
    ModalContext, NetworkStatus, ToastContainer, ToastContext,
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
    // CRITICAL: Use StoredValue to ensure contexts are created ONLY ONCE.
    // Without this, every App re-render creates new contexts with new global
    // event listeners, causing catastrophic performance degradation (exponential
    // listener accumulation). See: WEBAPP_REFACTORING_PLAN.md for details.

    let app_state = StoredValue::new(AppState::new());
    provide_context(app_state.get_value());

    let toast_ctx = StoredValue::new(ToastContext::new());
    provide_context(toast_ctx.get_value());

    let modal_ctx = StoredValue::new(ModalContext::new());
    provide_context(modal_ctx.get_value());

    let kb_ctx = StoredValue::new(KeyboardContext::new());
    provide_context(kb_ctx.get_value());

    // Get app_state for use in view (no cloning needed, it's cheap to get)
    let app_state = app_state.get_value();

    // Clone app_state for each closure that needs it (AppState is cheap to clone)
    let app_state_nav1 = app_state.clone();
    let app_state_nav2 = app_state.clone();
    let app_state_profile = app_state.clone();
    let app_state_admin = app_state.clone();
    let app_state_auth = app_state.clone();
    let app_state_logout = app_state.clone();

    view! {
        <Router>
            <ToastContainer />
            <ModalContainer />
            <KeyboardListener />
            <KeyboardShortcutsHelp />
            <NetworkStatus />
            <main>
                <nav class="main-nav">
                    <div class="nav-container">
                        <a href="/" class="nav-brand">"Automatafl"</a>
                        <div class="nav-links">
                            <a href="/games">"Games"</a>
                            <Show when=move || app_state_nav1.is_authenticated.get()>
                                <a href="/games/create">"Create Game"</a>
                                <a href="/matchmaking">"Quick Match"</a>
                            </Show>
                            <a href="/leaderboard">"Leaderboard"</a>
                            <Show when=move || app_state_nav2.is_authenticated.get()>
                                {{
                                    move || {
                                        let player_id = app_state_profile.current_player_id.get();
                                        player_id.map(|id| view! {
                                            <a href=format!("/users/{}", id)>"Profile"</a>
                                        })
                                    }
                                }}
                            </Show>
                            <Show when=move || app_state_admin.is_admin()>
                                <a href="/admin">"Admin"</a>
                            </Show>
                            <a href="/health">"Status"</a>
                        </div>
                        <div class="nav-auth">
                            <ConnectionStatus />
                            <Show
                                when=move || app_state_auth.is_authenticated.get()
                                fallback=|| view! {
                                    <a href="/login" class="button button-small">"Login"</a>
                                    <a href="/register" class="button button-small button-primary">"Register"</a>
                                }
                            >
                                <LogoutButton app_state=app_state_logout.clone() />
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
