// AdminPanel component - admin functionality for managing games and players
use crate::{api::ApiClient, state::AppState};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn AdminPanel() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (players, set_players) = signal(Vec::<PlayerListItem>::new());
    let (games, set_games) = signal(Vec::<GameListItem>::new());
    let (status, set_status) = signal(String::new());
    let (active_tab, set_active_tab) = signal("players".to_string());
    
    let api_base_url = app_state.api_base_url.clone();
    
    // Load players action
    let api_base_url_for_players = api_base_url.clone();
    let load_players_action = Action::new_local(move |_: &()| {
        let base_url = api_base_url_for_players.clone();
        async move {
            let client = ApiClient::new(base_url);
            match client.admin_list_players().await {
                Ok(value) => {
                    if let Ok(players) = serde_json::from_value::<Vec<PlayerListItem>>(value) {
                        Ok(players)
                    } else {
                        Ok(vec![])
                    }
                }
                Err(e) => Err(e.to_string())
            }
        }
    });
    
    // Load games action
    let api_base_url_for_games = api_base_url.clone();
    let load_games_action = Action::new_local(move |_: &()| {
        let base_url = api_base_url_for_games.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.list_games().await.map_err(|e| e.to_string())
        }
    });
    
    // Delete game action
    let api_base_url_for_delete = api_base_url.clone();
    let delete_game_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url_for_delete.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_delete_game(gid).await.map_err(|e| e.to_string())
        }
    });
    
    // Force complete round action
    let api_base_url_for_force = api_base_url.clone();
    let force_complete_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url_for_force.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_force_complete_round(gid).await.map(|_| ()).map_err(|e| e.to_string())
        }
    });
    
    // Handle results
    Effect::new(move |_| {
        if let Some(result) = load_players_action.value().get() {
            match result {
                Ok(player_list) => set_players.set(player_list),
                Err(e) => set_status.set(format!("Failed to load players: {}", e)),
            }
        }
    });
    
    Effect::new(move |_| {
        if let Some(result) = load_games_action.value().get() {
            match result {
                Ok(game_list) => set_games.set(game_list),
                Err(e) => set_status.set(format!("Failed to load games: {}", e)),
            }
        }
    });
    
    Effect::new(move |_| {
        if let Some(result) = delete_game_action.value().get() {
            match result {
                Ok(_) => {
                    set_status.set("Game deleted successfully".to_string());
                    load_games_action.dispatch(());
                }
                Err(e) => set_status.set(format!("Failed to delete game: {}", e)),
            }
        }
    });
    
    Effect::new(move |_| {
        if let Some(result) = force_complete_action.value().get() {
            match result {
                Ok(_) => {
                    set_status.set("Round completed successfully".to_string());
                    load_games_action.dispatch(());
                }
                Err(e) => set_status.set(format!("Failed to complete round: {}", e)),
            }
        }
    });
    
    // Load data on mount
    Effect::new(move |_| {
        let _ = load_players_action.dispatch(());
        let _ = load_games_action.dispatch(());
    });
    
    view! {
        <div class="admin-panel">
            <h2>"🔧 Admin Panel"</h2>
            
            <div class="admin-tabs">
                <button
                    class=move || format!("tab {}", if active_tab.get() == "players" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("players".to_string())
                >
                    "Players"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "games" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("games".to_string())
                >
                    "Games"
                </button>
            </div>
            
            <Show when=move || active_tab.get() == "players">
                <div class="admin-section">
                    <div class="section-header">
                        <h3>"Player Management"</h3>
                        <button
                            class="button button-small"
                            on:click=move |_| { let _ = load_players_action.dispatch(()); }
                        >
                            "Refresh"
                        </button>
                    </div>
                    
                    <div class="players-list">
                        {move || {
                            let player_list = players.get();
                            if player_list.is_empty() {
                                view! {
                                    <div class="empty-state">
                                        <p>"No players found or admin endpoints not implemented"</p>
                                    </div>
                                }.into_any()
                            } else {
                                player_list.into_iter().map(|player| {
                                    view! {
                                        <div class="player-item">
                                            <div class="player-info">
                                                <span class="player-name">{player.displayname}</span>
                                                <span class="player-id">{player.id.to_string()}</span>
                                                {if player.is_admin {
                                                    view! { <span class="admin-badge">"Admin"</span> }.into_any()
                                                } else {
                                                    view! {}.into_any()
                                                }}
                                            </div>
                                        </div>
                                    }
                                }).collect_view().into_any()
                            }
                        }}
                    </div>
                </div>
            </Show>
            
            <Show when=move || active_tab.get() == "games">
                <div class="admin-section">
                    <div class="section-header">
                        <h3>"Game Management"</h3>
                        <button
                            class="button button-small"
                            on:click=move |_| { let _ = load_games_action.dispatch(()); }
                        >
                            "Refresh"
                        </button>
                    </div>
                    
                    <div class="games-list">
                        {move || {
                            let game_list = games.get();
                            if game_list.is_empty() {
                                view! {
                                    <div class="empty-state">
                                        <p>"No games found"</p>
                                    </div>
                                }.into_any()
                            } else {
                                game_list.into_iter().map(|game| {
                                    let game_id = game.id;
                                    view! {
                                        <div class="game-item">
                                            <div class="game-info">
                                                <span class="game-id">{game.id.to_string().chars().take(8).collect::<String>()}</span>
                                                <span class="game-status">{format!("{:?}", game.lifecycle)}</span>
                                                <span class="game-players">{game.player_count}"/"{game.max_players}</span>
                                            </div>
                                            <div class="game-actions">
                                                <button
                                                    class="button button-small button-primary"
                                                    on:click=move |_| { let _ = force_complete_action.dispatch(game_id); }
                                                    disabled=move || force_complete_action.pending().get()
                                                >
                                                    "Force Complete"
                                                </button>
                                                <button
                                                    class="button button-small button-danger"
                                                    on:click=move |_| {
                                                        if web_sys::window().unwrap().confirm_with_message("Are you sure you want to delete this game?").unwrap_or(false) {
                                                            let _ = delete_game_action.dispatch(game_id);
                                                        }
                                                    }
                                                    disabled=move || delete_game_action.pending().get()
                                                >
                                                    "Delete"
                                                </button>
                                            </div>
                                        </div>
                                    }
                                }).collect_view().into_any()
                            }
                        }}
                    </div>
                </div>
            </Show>
            
            {move || {
                let s = status.get();
                if !s.is_empty() {
                    view! {
                        <div class="admin-status">{s}</div>
                    }.into_any()
                } else {
                    view! {}.into_any()
                }
            }}
        </div>
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
struct PlayerListItem {
    id: Uuid,
    displayname: String,
    is_admin: bool,
}

// Re-export from api types
use automatafl_api_types::GameListItem;

