mod api;
mod auth;
mod board;
mod chat;
mod game_lobby;
mod game_view;
mod websocket;

use api::ApiClient;
use auth::{AuthSection, AuthState};
use game_lobby::GameLobby;
use game_view::GameView;
use leptos::prelude::*;
use std::sync::Arc;
use uuid::Uuid;
use websocket::WebSocketConnection;

#[derive(Clone, PartialEq)]
enum AppView {
    Lobby,
    Game(Uuid),
    Profile(Uuid),
    Leaderboard,
    History,
    Matchmaking,
}

#[component]
fn App() -> impl IntoView {
    // Initialize auth state
    let auth = AuthState::new();
    provide_context(auth.clone());

    // Initialize API client
    let api = ApiClient::new(auth.token.clone());
    provide_context(api.clone());

    // Initialize WebSocket connection
    let (ws, ws_msg) = WebSocketConnection::new();
    let (_, set_ws_msg) = create_signal(None);
    provide_context(ws.clone());
    provide_context(ws_msg);

    // App navigation state
    let (current_view, set_current_view) = create_signal(AppView::Lobby);

    // Connect WebSocket when authenticated
    create_effect({
        let auth = auth.clone();
        let ws = ws.clone();
        move |_| {
            if auth.is_authenticated() {
                let token = auth.token.get();
                let ws = ws.clone();
                spawn_local(async move {
                    ws.connect(token, set_ws_msg).await;
                });
            }
        }
    });

    // Load user info if we have a token
    create_effect({
        let auth = auth.clone();
        let api = api.clone();
        move |_| {
            if auth.token.get().is_some() && auth.user.get().is_none() {
                let auth = auth.clone();
                let api = api.clone();
                spawn_local(async move {
                    match api.get_me().await {
                        Ok(user) => {
                            auth.set_user.set(Some(user));
                        }
                        Err(_) => {
                            // Token is invalid, clear it
                            auth.logout();
                        }
                    }
                });
            }
        }
    });

    let navigate_to = move |view: AppView| {
        set_current_view.set(view);
    };

    view! {
        <div class="container">
            <header class="app-header">
                <h1 on:click=move |_| navigate_to(AppView::Lobby)>"Automatafl"</h1>
                <nav class="main-nav">
                    <button
                        class={move || if current_view.get() == AppView::Lobby { "active" } else { "" }}
                        on:click=move |_| navigate_to(AppView::Lobby)
                    >
                        "Lobby"
                    </button>
                    <button
                        class={move || if current_view.get() == AppView::Leaderboard { "active" } else { "" }}
                        on:click=move |_| navigate_to(AppView::Leaderboard)
                    >
                        "Leaderboard"
                    </button>
                    <button
                        class={move || if current_view.get() == AppView::History { "active" } else { "" }}
                        on:click=move |_| navigate_to(AppView::History)
                    >
                        "History"
                    </button>
                    <button
                        class={move || if current_view.get() == AppView::Matchmaking { "active" } else { "" }}
                        on:click=move |_| navigate_to(AppView::Matchmaking)
                    >
                        "Matchmaking"
                    </button>
                    {move || auth.user.get().map(|user| view! {
                        <button
                            class={move || if matches!(current_view.get(), AppView::Profile(_)) { "active" } else { "" }}
                            on:click=move |_| navigate_to(AppView::Profile(user.id))
                        >
                            "Profile"
                        </button>
                    })}
                </nav>
                <div class="auth-container">
                    <AuthSection auth=auth.clone() api=api.clone() />
                </div>
            </header>

            <main class="app-main">
                {move || if auth.is_authenticated() || matches!(current_view.get(), AppView::Lobby | AppView::Leaderboard) {
                    match current_view.get() {
                        AppView::Lobby => view! {
                            <GameLobby
                                api=api.clone()
                                on_select_game=move |game_id| navigate_to(AppView::Game(game_id))
                            />
                        }.into_view(),
                        AppView::Game(game_id) => view! {
                            <GameView
                                game_id=game_id
                                api=api.clone()
                                auth=auth.clone()
                                ws=ws.clone()
                                on_leave_game=move || navigate_to(AppView::Lobby)
                            />
                        }.into_view(),
                        AppView::Profile(user_id) => view! {
                            <div class="profile-view">
                                <h2>"User Profile"</h2>
                                <p>"Profile for user: " {user_id.to_string()}</p>
                                <p>"(Profile view coming soon...)"</p>
                            </div>
                        }.into_view(),
                        AppView::Leaderboard => view! {
                            <div class="leaderboard-view">
                                <h2>"Leaderboard"</h2>
                                <p>"(Leaderboard coming soon...)"</p>
                            </div>
                        }.into_view(),
                        AppView::History => view! {
                            <div class="history-view">
                                <h2>"Game History"</h2>
                                <p>"(History view coming soon...)"</p>
                            </div>
                        }.into_view(),
                        AppView::Matchmaking => view! {
                            <div class="matchmaking-view">
                                <h2>"Matchmaking"</h2>
                                <p>"(Matchmaking coming soon...)"</p>
                            </div>
                        }.into_view(),
                    }
                } else {
                    view! {
                        <div class="auth-required">
                            <h2>"Authentication Required"</h2>
                            <p>"Please log in or register to access this feature."</p>
                        </div>
                    }.into_view()
                }}
            </main>
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(|| view! { <App /> });
}
