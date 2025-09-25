use crate::{api::ApiClient, state::AppState};
use automatafl_api::{UserProfileResponse, UserGamesResponse, GameSummary};
use leptos::prelude::*;
use leptos_router::{hooks::use_params_map, components::A};
use uuid::Uuid;

#[component]
pub fn UserProfilePage() -> impl IntoView {
    let params = use_params_map();
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let user_id = move || {
        params.get()
            .get("id")
            .and_then(|id| Uuid::parse_str(&id).ok())
    };
    
    let (profile, set_profile) = create_signal(Option::<UserProfileResponse>::None);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(Option::<String>::None);
    
    // Fetch user profile
    Effect::new(move |_| {
        if let Some(id) = user_id() {
            spawn_local(async move {
                let client = ApiClient::new(
                    app_state.api_base_url.clone(),
                    app_state.auth_token.get()
                );
                
                match client.get_user_profile(id).await {
                    Ok(profile_data) => {
                        set_profile.set(Some(profile_data));
                        set_loading.set(false);
                    }
                    Err(e) => {
                        set_error.set(Some(e));
                        set_loading.set(false);
                    }
                }
            });
        }
    });

    view! {
        <div class="user-profile-page">
            <Show
                when=move || loading.get()
                fallback=move || view! {
                    <Show
                        when=move || profile.get().is_some()
                        fallback=move || view! {
                            <div class="error-state">
                                <h2>"User not found"</h2>
                                <p>{move || error.get().unwrap_or_else(|| "Failed to load user profile".to_string())}</p>
                                <A href="/leaderboard" attr:class="button">"View Leaderboard"</A>
                            </div>
                        }
                    >
                        {move || {
                            let profile_data = profile.get().unwrap();
                            let user = &profile_data.user;
                            let stats = &profile_data.stats;
                            
                            view! {
                                <div class="profile-container">
                                    <div class="profile-header">
                                        <h1>{user.username.clone()}</h1>
                                        <div class="profile-badges">
                                            <span class="rating-badge">
                                                "Rating: " <strong>{user.rating}</strong>
                                            </span>
                                            {move || {
                                                if user.rating >= 2000 {
                                                    Some(view! { <span class="elite-badge">"🏆 Elite Player"</span> })
                                                } else if user.rating >= 1500 {
                                                    Some(view! { <span class="advanced-badge">"⭐ Advanced"</span> })
                                                } else {
                                                    None
                                                }
                                            }}
                                        </div>
                                    </div>
                                    
                                    <div class="profile-stats">
                                        <h2>"Statistics"</h2>
                                        <div class="stats-grid">
                                            <div class="stat-card">
                                                <span class="stat-value">{stats.total_games}</span>
                                                <span class="stat-label">"Total Games"</span>
                                            </div>
                                            <div class="stat-card wins">
                                                <span class="stat-value">{stats.wins}</span>
                                                <span class="stat-label">"Wins"</span>
                                            </div>
                                            <div class="stat-card losses">
                                                <span class="stat-value">{stats.losses}</span>
                                                <span class="stat-label">"Losses"</span>
                                            </div>
                                            <div class="stat-card draws">
                                                <span class="stat-value">{stats.draws}</span>
                                                <span class="stat-label">"Draws"</span>
                                            </div>
                                            <div class="stat-card">
                                                <span class="stat-value">{format!("{:.1}%", stats.win_rate)}</span>
                                                <span class="stat-label">"Win Rate"</span>
                                            </div>
                                            <div class="stat-card">
                                                <span class="stat-value">
                                                    {if stats.current_streak > 0 {
                                                        format!("+{}", stats.current_streak)
                                                    } else {
                                                        stats.current_streak.to_string()
                                                    }}
                                                </span>
                                                <span class="stat-label">"Current Streak"</span>
                                            </div>
                                        </div>
                                        
                                        <div class="rating-history">
                                            <h3>"Rating Range"</h3>
                                            <div class="rating-range">
                                                <span class="rating-low">
                                                    "Lowest: " <strong>{stats.lowest_rating}</strong>
                                                </span>
                                                <span class="rating-current">
                                                    "Current: " <strong>{user.rating}</strong>
                                                </span>
                                                <span class="rating-high">
                                                    "Highest: " <strong>{stats.highest_rating}</strong>
                                                </span>
                                            </div>
                                        </div>
                                    </div>
                                    
                                    <div class="recent-games">
                                        <h2>"Recent Games"</h2>
                                        <div class="games-list">
                                            <For
                                                each=move || profile_data.recent_games.clone()
                                                key=|game| game.id
                                                let:game
                                            >
                                                <GameSummaryCard game=game user_id=user.id />
                                            </For>
                                        </div>
                                        
                                        <div class="view-all-games">
                                            <A href=format!("/users/{}/games", user.id) attr:class="button">
                                                "View All Games"
                                            </A>
                                        </div>
                                    </div>
                                </div>
                            }
                        }}
                    </Show>
                }
            >
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading profile..."</p>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn GameSummaryCard(game: GameSummary, user_id: Uuid) -> impl IntoView {
    let is_white = game.white_player.as_ref().map(|p| p.id == user_id).unwrap_or(false);
    let is_black = game.black_player.as_ref().map(|p| p.id == user_id).unwrap_or(false);
    let won = game.winner.as_ref().map(|w| w.id == user_id).unwrap_or(false);
    
    let result_class = if won {
        "result-win"
    } else if game.winner.is_none() {
        "result-draw"
    } else {
        "result-loss"
    };
    
    let result_text = if won {
        "Won"
    } else if game.winner.is_none() {
        "Draw"
    } else {
        "Lost"
    };
    
    view! {
        <div class=format!("game-summary-card {}", result_class)>
            <div class="game-summary-header">
                <span class="game-date">
                    {game.created_at.format("%Y-%m-%d %H:%M").to_string()}
                </span>
                <span class="game-result">{result_text}</span>
            </div>
            
            <div class="game-summary-players">
                <div class="player" class:current-player=is_white>
                    <span class="player-color">"⚪"</span>
                    {game.white_player.as_ref()
                        .map(|p| p.username.clone())
                        .unwrap_or_else(|| "Unknown".to_string())}
                </div>
                <span class="vs">"vs"</span>
                <div class="player" class:current-player=is_black>
                    <span class="player-color">"⚫"</span>
                    {game.black_player.as_ref()
                        .map(|p| p.username.clone())
                        .unwrap_or_else(|| "Unknown".to_string())}
                </div>
            </div>
            
            <div class="game-summary-info">
                <span class="move-count">{game.total_moves} " moves"</span>
                <A href=format!("/games/{}/history", game.id) attr:class="view-game-link">
                    "View Game →"
                </A>
            </div>
        </div>
    }
}

#[component]
pub fn UserGamesPage() -> impl IntoView {
    let params = use_params_map();
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let user_id = move || {
        params.get()
            .get("id")
            .and_then(|id| Uuid::parse_str(&id).ok())
    };
    
    let (games_response, set_games_response) = create_signal(Option::<UserGamesResponse>::None);
    let (current_page, set_current_page) = create_signal(1);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(Option::<String>::None);
    
    // Fetch games
    let fetch_games = move |page: i32| {
        if let Some(id) = user_id() {
            set_loading.set(true);
            set_error.set(None);
            
            spawn_local(async move {
                let client = ApiClient::new(
                    app_state.api_base_url.clone(),
                    app_state.auth_token.get()
                );
                
                match client.get_user_games(id, page).await {
                    Ok(response) => {
                        set_games_response.set(Some(response));
                        set_loading.set(false);
                    }
                    Err(e) => {
                        set_error.set(Some(e));
                        set_loading.set(false);
                    }
                }
            });
        }
    };
    
    // Initial fetch
    Effect::new(move |_| {
        fetch_games(current_page.get());
    });

    view! {
        <div class="user-games-page">
            <div class="page-header">
                <h1>"Game History"</h1>
                <Show when=move || user_id().is_some()>
                    <A href=format!("/users/{}", user_id().unwrap()) attr:class="button button-small">
                        "← Back to Profile"
                    </A>
                </Show>
            </div>
            
            <Show
                when=move || loading.get()
                fallback=move || view! {
                    <Show
                        when=move || games_response.get().is_some()
                        fallback=move || view! {
                            <div class="error-state">
                                <h3>"Error loading games"</h3>
                                <p>{move || error.get().unwrap_or_default()}</p>
                                <button attr:class="button" on:click=move |_| fetch_games(current_page.get())>
                                    "Retry"
                                </button>
                            </div>
                        }
                    >
                        {move || {
                            let response = games_response.get().unwrap();
                            let user_id = user_id().unwrap();
                            
                            view! {
                                <div class="games-history-container">
                                    <div class="games-stats">
                                        <span>"Total games: " <strong>{response.total_games}</strong></span>
                                        <span>" | Page " {response.page} " of " 
                                            {(response.total_games / response.per_page as i64 + 1)}
                                        </span>
                                    </div>
                                    
                                    <div class="games-list">
                                        <For
                                            each=move || response.games.clone()
                                            key=|game| game.id
                                            let:game
                                        >
                                            <GameSummaryCard game=game user_id=user_id />
                                        </For>
                                    </div>
                                    
                                    // Pagination
                                    <div class="pagination">
                                        <button
                                            attr:class="button button-small"
                                            disabled=move || current_page.get() <= 1
                                            on:click=move |_| {
                                                let new_page = current_page.get() - 1;
                                                set_current_page.set(new_page);
                                                fetch_games(new_page);
                                            }
                                        >
                                            "Previous"
                                        </button>
                                        
                                        <span class="page-info">
                                            "Page " {move || current_page.get()}
                                        </span>
                                        
                                        <button
                                            attr:class="button button-small"
                                            disabled=move || {
                                                let total_pages = response.total_games / response.per_page as i64 + 1;
                                                current_page.get() >= total_pages as i32
                                            }
                                            on:click=move |_| {
                                                let new_page = current_page.get() + 1;
                                                set_current_page.set(new_page);
                                                fetch_games(new_page);
                                            }
                                        >
                                            "Next"
                                        </button>
                                    </div>
                                </div>
                            }
                        }}
                    </Show>
                }
            >
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading games..."</p>
                </div>
            </Show>
        </div>
    }
}
