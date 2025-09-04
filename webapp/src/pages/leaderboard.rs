use crate::{api::ApiClient, state::AppState};
use automatafl_api::{LeaderboardEntry, LeaderboardResponse};
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn LeaderboardPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (leaderboard, set_leaderboard) = create_signal(Option::<LeaderboardResponse>::None);
    let (current_page, set_current_page) = create_signal(1);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(Option::<String>::None);
    
    // Fetch leaderboard data
    let fetch_leaderboard = move |page: i32| {
        set_loading(true);
        set_error(None);
        
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.get_leaderboard(page).await {
                Ok(response) => {
                    set_leaderboard(Some(response));
                    set_loading(false);
                }
                Err(e) => {
                    set_error(Some(e));
                    set_loading(false);
                }
            }
        });
    };
    
    // Fetch initial data
    Effect::new(move |_| {
        fetch_leaderboard(current_page.get());
    });

    view! {
        <div class="leaderboard-page">
            <div class="page-header">
                <h1>"🏆 Leaderboard"</h1>
                <p class="page-subtitle">"Top players ranked by rating"</p>
            </div>
            
            <Show
                when=move || loading.get()
                fallback=move || view! {
                    <Show
                        when=move || error.get().is_some()
                        fallback=move || view! {
                            {move || {
                                if let Some(lb) = leaderboard.get() {
                                    view! {
                                        <div class="leaderboard-container">
                                            <div class="leaderboard-stats">
                                                <div class="stat-card">
                                                    <span class="stat-label">"Total Players"</span>
                                                    <span class="stat-value">{lb.total}</span>
                                                </div>
                                                <div class="stat-card">
                                                    <span class="stat-label">"Page"</span>
                                                    <span class="stat-value">{lb.page} "/" {(lb.total / lb.per_page as i64) + 1}</span>
                                                </div>
                                            </div>
                                            
                                            <div class="leaderboard-table">
                                                <div class="table-header">
                                                    <div class="table-cell rank-cell">"Rank"</div>
                                                    <div class="table-cell player-cell">"Player"</div>
                                                    <div class="table-cell rating-cell">"Rating"</div>
                                                    <div class="table-cell wins-cell">"Wins"</div>
                                                    <div class="table-cell losses-cell">"Losses"</div>
                                                    <div class="table-cell draws-cell">"Draws"</div>
                                                    <div class="table-cell winrate-cell">"Win Rate"</div>
                                                </div>
                                                
                                                <For
                                                    each=move || lb.entries.clone()
                                                    key=|entry| entry.user.id
                                                    let:entry
                                                >
                                                    <LeaderboardRow entry=entry />
                                                </For>
                                            </div>
                                            
                                            // Pagination
                                            <div class="pagination">
                                                <button
                                                    class="button button-small"
                                                    disabled=move || current_page.get() <= 1
                                                    on:click=move |_| {
                                                        let new_page = current_page.get() - 1;
                                                        set_current_page(new_page);
                                                        fetch_leaderboard(new_page);
                                                    }
                                                >
                                                    "Previous"
                                                </button>
                                                
                                                <span class="page-info">
                                                    "Page " {move || current_page.get()} " of " 
                                                    {move || {
                                                        if let Some(lb) = leaderboard.get() {
                                                            ((lb.total / lb.per_page as i64) + 1).to_string()
                                                        } else {
                                                            "?".to_string()
                                                        }
                                                    }}
                                                </span>
                                                
                                                <button
                                                    class="button button-small"
                                                    disabled=move || {
                                                        if let Some(lb) = leaderboard.get() {
                                                            current_page.get() >= ((lb.total / lb.per_page as i64) + 1) as i32
                                                        } else {
                                                            true
                                                        }
                                                    }
                                                    on:click=move |_| {
                                                        let new_page = current_page.get() + 1;
                                                        set_current_page(new_page);
                                                        fetch_leaderboard(new_page);
                                                    }
                                                >
                                                    "Next"
                                                </button>
                                            </div>
                                        </div>
                                    }
                                } else {
                                    view! {
                                        <div class="empty-state">
                                            <p>"No leaderboard data available"</p>
                                        </div>
                                    }
                                }
                            }}
                        }
                    >
                        <div class="error-state">
                            <h3>"Error loading leaderboard"</h3>
                            <p>{move || error.get().unwrap_or_default()}</p>
                            <button class="button" on:click=move |_| fetch_leaderboard(current_page.get())>
                                "Retry"
                            </button>
                        </div>
                    </Show>
                }
            >
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading leaderboard..."</p>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn LeaderboardRow(entry: LeaderboardEntry) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let current_user = app_state.current_user;
    
    let is_current_user = move || {
        current_user.get()
            .map(|user| user.id == entry.user.id)
            .unwrap_or(false)
    };
    
    let total_games = entry.wins + entry.losses + entry.draws;
    let win_rate = if total_games > 0 {
        (entry.wins as f32 / total_games as f32 * 100.0) as u32
    } else {
        0
    };
    
    let rank_class = match entry.rank {
        1 => "rank-gold",
        2 => "rank-silver",
        3 => "rank-bronze",
        _ => "rank-normal",
    };
    
    view! {
        <div class="table-row" class:current-user-row=is_current_user>
            <div class=format!("table-cell rank-cell {}", rank_class)>
                {match entry.rank {
                    1 => "🥇",
                    2 => "🥈",
                    3 => "🥉",
                    _ => "",
                }}
                {entry.rank}
            </div>
            <div class="table-cell player-cell">
                <A href=format!("/users/{}", entry.user.id) class="player-link">
                    {entry.user.username}
                </A>
                <Show when=is_current_user>
                    <span class="you-badge">" (You)"</span>
                </Show>
            </div>
            <div class="table-cell rating-cell">
                <span class="rating-value">{entry.user.rating}</span>
            </div>
            <div class="table-cell wins-cell">
                <span class="wins-value">{entry.wins}</span>
            </div>
            <div class="table-cell losses-cell">
                <span class="losses-value">{entry.losses}</span>
            </div>
            <div class="table-cell draws-cell">
                <span class="draws-value">{entry.draws}</span>
            </div>
            <div class="table-cell winrate-cell">
                <div class="winrate-bar">
                    <div 
                        class="winrate-fill"
                        style=move || format!("width: {}%", win_rate)
                    ></div>
                    <span class="winrate-text">{win_rate}"%"</span>
                </div>
            </div>
        </div>
    }
}
