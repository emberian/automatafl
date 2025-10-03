// AdminPanel component - comprehensive admin functionality
use crate::{api::ApiClient, state::AppState, components::{use_modal, use_toast}};
use leptos::prelude::*;
use uuid::Uuid;
use automatafl_api_types::{PlayerListItem, GameListItem};

#[component]
pub fn AdminPanel() -> impl IntoView {
    let (active_tab, set_active_tab) = signal("players".to_string());
    let (status, set_status) = signal(String::new());
    
    view! {
        <div class="admin-panel">
            <h2>"🔧 Admin Panel"</h2>
            
            <div class="admin-tabs">
                <button
                    class=move || format!("tab {}", if active_tab.get() == "players" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("players".to_string())
                >
                    "👥 Players"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "games" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("games".to_string())
                >
                    "🎮 Games"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "sessions" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("sessions".to_string())
                >
                    "🔑 Sessions"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "matchmaking" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("matchmaking".to_string())
                >
                    "⚔️ Matchmaking"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "database" { "active" } else { "" })
                    on:click=move |_| set_active_tab.set("database".to_string())
                >
                    "💾 Database"
                </button>
            </div>
            
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
            
            <Show when=move || active_tab.get() == "players">
                <AdminPlayersTab status=status _set_status=set_status />
            </Show>
            
            <Show when=move || active_tab.get() == "games">
                <AdminGamesTab status=status _set_status=set_status />
            </Show>
            
            <Show when=move || active_tab.get() == "sessions">
                <AdminSessionsTab status=status _set_status=set_status />
            </Show>
            
            <Show when=move || active_tab.get() == "matchmaking">
                <AdminMatchmakingTab status=status _set_status=set_status />
            </Show>
            
            <Show when=move || active_tab.get() == "database">
                <AdminDatabaseTab status=status _set_status=set_status />
            </Show>
        </div>
    }
}

// ============================================================================
// Players Tab
// ============================================================================

#[component]
fn AdminPlayersTab(
    #[allow(unused_variables)] status: ReadSignal<String>,
    _set_status: WriteSignal<String>
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let modal = use_modal();
    let toast = use_toast();
    
    let (players, set_players) = signal(Vec::<PlayerListItem>::new());
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let api_base_url = app_state.api_base_url.clone();
    let load_players_action = Action::new_local(move |_: &()| {
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_list_players().await.map_err(|e| e.to_string())
        }
    });
    
    let api_base_url_for_delete = app_state.api_base_url.clone();
    let delete_player_action = Action::new_local(move |pid: &Uuid| {
        let pid = *pid;
        let base_url = api_base_url_for_delete.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_delete_player(pid).await.map_err(|e| e.to_string())
        }
    });
    
    let _ = Effect::new(move |_| {
        let _ = refresh_trigger.get();
        load_players_action.dispatch(());
    });
    
    let toast_clone = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = load_players_action.value().get() {
            match result {
                Ok(player_list) => set_players.set(player_list),
                Err(e) => toast_clone.error(format!("Failed to load players: {}", e)),
            }
        }
    });
    
    let toast_clone2 = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = delete_player_action.value().get() {
            match result {
                Ok(_) => {
                    toast_clone2.success("Player deleted successfully");
                    set_refresh_trigger.update(|n| *n += 1);
                }
                Err(e) => toast_clone2.error(format!("Failed to delete player: {}", e)),
            }
        }
    });
    
    view! {
        <div class="admin-section">
            <div class="section-header">
                <h3>"Player Management"</h3>
                <button
                    class="button button-small"
                    on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                >
                    "🔄 Refresh"
                </button>
            </div>
            
            <div class="players-table-container">
                <Suspense fallback=move || view! {
                    <div class="loading">"Loading players..."</div>
                }>
                    {move || {
                        let modal = modal.clone();
                        let player_list = players.get();
                        if player_list.is_empty() {
                            view! {
                                <div class="empty-state">
                                    <p>"No players found"</p>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <table class="admin-table">
                                    <thead>
                                        <tr>
                                            <th>"Display Name"</th>
                                            <th>"Player ID"</th>
                                            <th>"Status"</th>
                                            <th>"Actions"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {player_list.into_iter().map(|player| {
                                            let player_id = player.id;
                                            let modal = modal.clone();
                                            view! {
                                                <tr>
                                                    <td>{player.displayname.clone()}</td>
                                                    <td class="monospace">{player_id.to_string().chars().take(8).collect::<String>()}"..."</td>
                                                    <td>
                                                        {if player.is_admin {
                                                            view! { <span class="badge badge-admin">"Admin"</span> }
                                                        } else {
                                                            view! { <span class="badge badge-player">"Player"</span> }
                                                        }}
                                                    </td>
                                                    <td class="actions">
                                                        <a href=format!("/users/{}", player_id) class="button button-small">"View"</a>
                                                        <button
                                                            class="button button-small button-danger"
                                                            on:click=move |_| {
                                                                let displayname = player.displayname.clone();
                                                                modal.confirm_danger(
                                                                    "Delete Player?",
                                                                    format!("Are you sure you want to delete {}? This action cannot be undone.", displayname),
                                                                    move || { delete_player_action.dispatch(player_id); }
                                                                );
                                                            }
                                                            disabled=move || delete_player_action.pending().get() || player.is_admin
                                                        >
                                                            "Delete"
                                                        </button>
                                                    </td>
                                                </tr>
                                            }
                                        }).collect_view()}
                                    </tbody>
                                </table>
                            }.into_any()
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

// ============================================================================
// Games Tab
// ============================================================================

#[component]
fn AdminGamesTab(
    #[allow(unused_variables)] status: ReadSignal<String>,
    _set_status: WriteSignal<String>
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let modal = use_modal();
    let toast = use_toast();
    
    let (games, set_games) = signal(Vec::<GameListItem>::new());
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let api_base_url = app_state.api_base_url.clone();
    let load_games_action = Action::new_local(move |_: &()| {
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_list_games().await.map_err(|e| e.to_string())
        }
    });
    
    let api_base_url_for_delete = app_state.api_base_url.clone();
    let delete_game_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url_for_delete.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_delete_game(gid).await.map_err(|e| e.to_string())
        }
    });
    
    let api_base_url_for_force = app_state.api_base_url.clone();
    let force_complete_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url_for_force.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_force_complete_round(gid).await.map(|_| ()).map_err(|e| e.to_string())
        }
    });
    
    let _ = Effect::new(move |_| {
        let _ = refresh_trigger.get();
        load_games_action.dispatch(());
    });
    
    let toast_clone = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = load_games_action.value().get() {
            match result {
                Ok(game_list) => set_games.set(game_list),
                Err(e) => toast_clone.error(format!("Failed to load games: {}", e)),
            }
        }
    });
    
    let toast_clone2 = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = delete_game_action.value().get() {
            match result {
                Ok(_) => {
                    toast_clone2.success("Game deleted successfully");
                    set_refresh_trigger.update(|n| *n += 1);
                }
                Err(e) => toast_clone2.error(format!("Failed to delete game: {}", e)),
            }
        }
    });
    
    let toast_clone3 = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = force_complete_action.value().get() {
            match result {
                Ok(_) => {
                    toast_clone3.success("Round completed successfully");
                    set_refresh_trigger.update(|n| *n += 1);
                }
                Err(e) => toast_clone3.error(format!("Failed to complete round: {}", e)),
            }
        }
    });
    
    view! {
        <div class="admin-section">
            <div class="section-header">
                <h3>"Game Management"</h3>
                <button
                    class="button button-small"
                    on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                >
                    "🔄 Refresh"
                </button>
            </div>
            
            <div class="games-table-container">
                <Suspense fallback=move || view! {
                    <div class="loading">"Loading games..."</div>
                }>
                    {move || {
                        let modal = modal.clone();
                        let game_list = games.get();
                        if game_list.is_empty() {
                            view! {
                                <div class="empty-state">
                                    <p>"No games found"</p>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <table class="admin-table">
                                    <thead>
                                        <tr>
                                            <th>"Game ID"</th>
                                            <th>"Status"</th>
                                            <th>"Players"</th>
                                            <th>"Created"</th>
                                            <th>"Actions"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {game_list.into_iter().map(|game| {
                                            let game_id = game.id;
                                            let modal = modal.clone();
                                            let lifecycle_str = format!("{:?}", game.lifecycle);
                                            let created_str = chrono::DateTime::from_timestamp(game.created_at as i64, 0)
                                                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                                .unwrap_or_else(|| "Unknown".to_string());

                                            view! {
                                                <tr>
                                                    <td class="monospace">{game.id.to_string().chars().take(8).collect::<String>()}"..."</td>
                                                    <td><span class="badge">{lifecycle_str}</span></td>
                                                    <td>{game.player_count}" / "{game.max_players}</td>
                                                    <td>{created_str}</td>
                                                    <td class="actions">
                                                        <a href=format!("/games/{}", game_id) class="button button-small">"View"</a>
                                                        <button
                                                            class="button button-small button-warning"
                                                            on:click=move |_| { let _ = force_complete_action.dispatch(game_id); }
                                                            disabled=move || force_complete_action.pending().get()
                                                        >
                                                            "Force Complete"
                                                        </button>
                                                        <button
                                                            class="button button-small button-danger"
                                                            on:click=move |_| {
                                                                modal.confirm_danger(
                                                                    "Delete Game?",
                                                                    "This will permanently delete the game and all its data. This action cannot be undone.",
                                                                    move || { delete_game_action.dispatch(game_id); }
                                                                );
                                                            }
                                                            disabled=move || delete_game_action.pending().get()
                                                        >
                                                            "Delete"
                                                        </button>
                                                    </td>
                                                </tr>
                                            }
                                        }).collect_view()}
                                    </tbody>
                                </table>
                            }.into_any()
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

// ============================================================================
// Sessions Tab
// ============================================================================

#[component]
fn AdminSessionsTab(
    #[allow(unused_variables)] status: ReadSignal<String>,
    _set_status: WriteSignal<String>
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();
    
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let api_base_url = app_state.api_base_url.clone();
    let sessions_resource = LocalResource::new(move || {
        let _ = refresh_trigger.get();
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_list_sessions().await
        }
    });
    
    let api_base_url_for_delete = app_state.api_base_url.clone();
    let delete_session_action = Action::new_local(move |sid: &Uuid| {
        let sid = *sid;
        let base_url = api_base_url_for_delete.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_delete_session(sid).await.map_err(|e| e.to_string())
        }
    });
    
    let api_base_url_for_cleanup = app_state.api_base_url.clone();
    let cleanup_action = Action::new_local(move |_: &()| {
        let base_url = api_base_url_for_cleanup.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_cleanup_expired_sessions().await.map_err(|e| e.to_string())
        }
    });
    
    let toast_clone = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = delete_session_action.value().get() {
            match result {
                Ok(_) => {
                    toast_clone.success("Session revoked successfully");
                    set_refresh_trigger.update(|n| *n += 1);
                }
                Err(e) => toast_clone.error(format!("Failed to revoke session: {}", e)),
            }
        }
    });

    let toast_clone2 = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = cleanup_action.value().get() {
            match result {
                Ok(_) => {
                    toast_clone2.success("Expired sessions cleaned up");
                    set_refresh_trigger.update(|n| *n += 1);
                }
                Err(e) => toast_clone2.error(format!("Failed to cleanup sessions: {}", e)),
            }
        }
    });
    
    view! {
        <div class="admin-section">
            <div class="section-header">
                <h3>"Session Management"</h3>
                <div class="header-actions">
                                        <button
                                            class="button button-small button-warning"
                                            on:click=move |_| { let _ = cleanup_action.dispatch(()); }
                                            disabled=move || cleanup_action.pending().get()
                                        >
                        "🧹 Cleanup Expired"
                    </button>
                    <button
                        class="button button-small"
                        on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                    >
                        "🔄 Refresh"
                    </button>
                </div>
            </div>
            
            <Suspense fallback=move || view! {
                <div class="loading">"Loading sessions..."</div>
            }>
                {move || {
                    sessions_resource.get().map(|result| {
                        match result {
                            Ok(sessions_value) => {
                                // Parse sessions from JSON
                                if let Ok(sessions) = serde_json::from_value::<Vec<serde_json::Value>>(sessions_value) {
                                    if sessions.is_empty() {
                                        view! {
                                            <div class="empty-state">
                                                <p>"No active sessions"</p>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <table class="admin-table">
                                                <thead>
                                                    <tr>
                                                        <th>"Session ID"</th>
                                                        <th>"Player"</th>
                                                        <th>"Expires At"</th>
                                                        <th>"Actions"</th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {sessions.into_iter().filter_map(|session| {
                                                        let session_id = session.get("id")?.as_str().and_then(|s| Uuid::parse_str(s).ok())?;
                                                        let player_name = session.get("player_displayname")?.as_str()?.to_string();
                                                        let expires_at = session.get("expires_at")?.as_u64()?;
                                                        
                                                        Some(view! {
                                                            <tr>
                                                                <td class="monospace">{session_id.to_string().chars().take(8).collect::<String>()}"..."</td>
                                                                <td>{player_name}</td>
                                                                <td>{chrono::DateTime::from_timestamp(expires_at as i64, 0)
                                                                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                                                    .unwrap_or_else(|| "Unknown".to_string())}</td>
                                                                <td class="actions">
                                                                    <button
                                                                        class="button button-small button-danger"
                                                                        on:click=move |_| { let _ = delete_session_action.dispatch(session_id); }
                                                                        disabled=move || delete_session_action.pending().get()
                                                                    >
                                                                        "Revoke"
                                                                    </button>
                                                                </td>
                                                            </tr>
                                                        })
                                                    }).collect_view()}
                                                </tbody>
                                            </table>
                                        }.into_any()
                                    }
                                } else {
                                    view! {
                                        <div class="error-state">"Failed to parse sessions"</div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! {
                                <div class="error-state">"Failed to load sessions: " {format!("{}", e)}</div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

// ============================================================================
// Matchmaking Tab
// ============================================================================

#[component]
fn AdminMatchmakingTab(
    #[allow(unused_variables)] status: ReadSignal<String>,
    _set_status: WriteSignal<String>
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();
    
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let api_base_url = app_state.api_base_url.clone();
    let queue_resource = LocalResource::new(move || {
        let _ = refresh_trigger.get();
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_list_matchmaking_queue().await
        }
    });
    
    let api_base_url_for_remove = app_state.api_base_url.clone();
    let remove_action = Action::new_local(move |pid: &Uuid| {
        let pid = *pid;
        let base_url = api_base_url_for_remove.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_remove_from_matchmaking(pid).await.map_err(|e| e.to_string())
        }
    });
    
    let toast_clone = toast.clone();
    let _ = Effect::new(move |_| {
        if let Some(result) = remove_action.value().get() {
            match result {
                Ok(_) => {
                    toast_clone.success("Player removed from queue");
                    set_refresh_trigger.update(|n| *n += 1);
                }
                Err(e) => toast_clone.error(format!("Failed to remove player: {}", e)),
            }
        }
    });
    
    view! {
        <div class="admin-section">
            <div class="section-header">
                <h3>"Matchmaking Queue"</h3>
                <button
                    class="button button-small"
                    on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                >
                    "🔄 Refresh"
                </button>
            </div>
            
            <Suspense fallback=move || view! {
                <div class="loading">"Loading matchmaking queue..."</div>
            }>
                {move || {
                    queue_resource.get().map(|result| {
                        match result {
                            Ok(queue_value) => {
                                if let Ok(queue) = serde_json::from_value::<Vec<serde_json::Value>>(queue_value) {
                                    if queue.is_empty() {
                                        view! {
                                            <div class="empty-state">
                                                <p>"No players in matchmaking queue"</p>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <table class="admin-table">
                                                <thead>
                                                    <tr>
                                                        <th>"Player"</th>
                                                        <th>"Queued At"</th>
                                                        <th>"Wait Time"</th>
                                                        <th>"Preferences"</th>
                                                        <th>"Actions"</th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {queue.into_iter().filter_map(|entry| {
                                                        let player_id = entry.get("player_id")?.as_str().and_then(|s| Uuid::parse_str(s).ok())?;
                                                        let player_name = entry.get("player_displayname")?.as_str()?.to_string();
                                                        let queued_at = entry.get("queued_at")?.as_u64()?;
                                                        let wait_time = entry.get("wait_time_seconds")?.as_u64()?;
                                                        let prefs = entry.get("game_preferences")?.clone();
                                                        
                                                        Some(view! {
                                                            <tr>
                                                                <td>{player_name}</td>
                                                                <td>{chrono::DateTime::from_timestamp(queued_at as i64, 0)
                                                                    .map(|dt| dt.format("%H:%M:%S").to_string())
                                                                    .unwrap_or_else(|| "Unknown".to_string())}</td>
                                                                <td>{wait_time}" seconds"</td>
                                                                <td class="monospace">{format!("{:?}", prefs)}</td>
                                                                <td class="actions">
                                                                    <button
                                                                        class="button button-small button-danger"
                                                                        on:click=move |_| { let _ = remove_action.dispatch(player_id); }
                                                                        disabled=move || remove_action.pending().get()
                                                                    >
                                                                        "Remove"
                                                                    </button>
                                                                </td>
                                                            </tr>
                                                        })
                                                    }).collect_view()}
                                                </tbody>
                                            </table>
                                        }.into_any()
                                    }
                                } else {
                                    view! {
                                        <div class="error-state">"Failed to parse queue data"</div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! {
                                <div class="error-state">"Failed to load queue: " {format!("{}", e)}</div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

// ============================================================================
// Database Tab
// ============================================================================

#[component]
fn AdminDatabaseTab(
    #[allow(unused_variables)] status: ReadSignal<String>,
    #[allow(unused_variables)] _set_status: WriteSignal<String>
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let api_base_url_stats = app_state.api_base_url.clone();
    let stats_resource = LocalResource::new(move || {
        let _ = refresh_trigger.get();
        let base_url = api_base_url_stats.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_get_database_stats().await
        }
    });
    
    let api_base_url_tables = app_state.api_base_url.clone();
    let tables_resource = LocalResource::new(move || {
        let _ = refresh_trigger.get();
        let base_url = api_base_url_tables.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.admin_list_tables().await
        }
    });
    
    view! {
        <div class="admin-section">
            <div class="section-header">
                <h3>"Database Information"</h3>
                <button
                    class="button button-small"
                    on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                >
                    "🔄 Refresh"
                </button>
            </div>
            
            <div class="database-stats">
                <h4>"📊 Statistics"</h4>
                <Suspense fallback=move || view! {
                    <div class="loading">"Loading statistics..."</div>
                }>
                    {move || {
                        stats_resource.get().map(|result| {
                            match result {
                                Ok(stats_value) => {
                                    view! {
                                        <div class="stats-grid">
                                            {if let Some(obj) = stats_value.as_object() {
                                                obj.iter().map(|(key, value)| {
                                                    view! {
                                                        <div class="stat-card">
                                                            <div class="stat-label">{key.clone()}</div>
                                                            <div class="stat-value">{format!("{}", value)}</div>
                                                        </div>
                                                    }
                                                }).collect_view().into_any()
                                            } else {
                                                view! { <p>"No stats available"</p> }.into_any()
                                            }}
                                        </div>
                                    }.into_any()
                                }
                                Err(e) => view! {
                                    <div class="error-state">"Failed to load stats: " {format!("{}", e)}</div>
                                }.into_any()
                            }
                        })
                    }}
                </Suspense>
            </div>
            
            <div class="database-tables">
                <h4>"📋 Tables"</h4>
                <Suspense fallback=move || view! {
                    <div class="loading">"Loading tables..."</div>
                }>
                    {move || {
                        tables_resource.get().map(|result| {
                            match result {
                                Ok(tables_value) => {
                                    if let Ok(tables) = serde_json::from_value::<Vec<serde_json::Value>>(tables_value) {
                                        view! {
                                            <table class="admin-table">
                                                <thead>
                                                    <tr>
                                                        <th>"Table Name"</th>
                                                        <th>"Record Count"</th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {tables.into_iter().filter_map(|table| {
                                                        let name = table.get("name")?.as_str()?.to_string();
                                                        let count = table.get("record_count")?.as_u64()?;
                                                        
                                                        Some(view! {
                                                            <tr>
                                                                <td class="monospace">{name}</td>
                                                                <td>{count}</td>
                                                            </tr>
                                                        })
                                                    }).collect_view()}
                                                </tbody>
                                            </table>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div class="error-state">"Failed to parse table data"</div>
                                        }.into_any()
                                    }
                                }
                                Err(e) => view! {
                                    <div class="error-state">"Failed to load tables: " {format!("{}", e)}</div>
                                }.into_any()
                            }
                        })
                    }}
                </Suspense>
            </div>
        </div>
    }
}
