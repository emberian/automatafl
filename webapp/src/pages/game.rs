use crate::{
    api::ApiClient,
    components::{GameBoard, ChatPanel, GameInfo, MoveControls},
    state::AppState,
    websocket::create_websocket_connection,
};
use automatafl_api::{GameStateResponse, GameStatus};
use leptos::prelude::*;
use leptos_router::{hooks::use_params_map, components::A};
use uuid::Uuid;

#[component]
pub fn GamesListPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let (games, set_games) = create_signal(Vec::<GameStateResponse>::new());
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(Option::<String>::None);
    
    // Fetch games on mount
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.list_games().await {
                Ok(game_list) => {
                    set_games(game_list);
                    set_loading(false);
                }
                Err(e) => {
                    set_error(Some(e));
                    set_loading(false);
                }
            }
        });
    });

    view! {
        <div class="games-list-page">
            <div class="page-header">
                <h1>"Active Games"</h1>
                <div class="header-actions">
                    <A href="/games/create" class="button button-primary">
                        "Create New Game"
                    </A>
                    <A href="/matchmaking" class="button button-secondary">
                        "Quick Match"
                    </A>
                </div>
            </div>
            
            <Show
                when=move || loading.get()
                fallback=move || view! {
                    <Show
                        when=move || error.get().is_some()
                        fallback=move || view! {
                            <div class="games-grid">
                                <For
                                    each=move || games.get()
                                    key=|game| game.id
                                    let:game
                                >
                                    <GameCard game=game />
                                </For>
                                
                                <Show when=move || games.get().is_empty()>
                                    <div class="empty-state">
                                        <h3>"No active games"</h3>
                                        <p>"Be the first to create a game!"</p>
                                    </div>
                                </Show>
                            </div>
                        }
                    >
                        <div class="error-state">
                            <h3>"Error loading games"</h3>
                            <p>{move || error.get().unwrap_or_default()}</p>
                            <button class="button" on:click=move |_| {
                                set_loading(true);
                                set_error(None);
                                // Trigger refetch by updating effect
                            }>
                                "Retry"
                            </button>
                        </div>
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

#[component]
fn GameCard(game: GameStateResponse) -> impl IntoView {
    let status_class = match game.round_state.as_str() {
        "waiting" => "status-waiting",
        "active" => "status-active",
        "completed" => "status-completed",
        _ => "status-unknown",
    };
    
    let status_text = match game.round_state.as_str() {
        "waiting" => "Waiting for players",
        "active" => "In progress",
        "completed" => "Completed",
        _ => "Unknown",
    };

    view! {
        <div class="game-card">
            <div class="game-card-header">
                <h3>"Game " {game.id.to_string().chars().take(8).collect::<String>()}</h3>
                <span class=format!("game-status {}", status_class)>{status_text}</span>
            </div>
            
            <div class="game-card-players">
                <div class="player-info">
                    <span class="player-label">"White:"</span>
                    {game.white_player.as_ref()
                        .map(|p| p.username.clone())
                        .unwrap_or_else(|| "Waiting...".to_string())}
                </div>
                <div class="player-info">
                    <span class="player-label">"Black:"</span>
                    {game.black_player.as_ref()
                        .map(|p| p.username.clone())
                        .unwrap_or_else(|| "Waiting...".to_string())}
                </div>
            </div>
            
            <Show when=move || game.spectator_count > 0>
                <div class="spectator-count">
                    "👁 " {game.spectator_count} " spectators"
                </div>
            </Show>
            
            <div class="game-card-actions">
                <A href=format!("/games/{}", game.id) class="button button-small">
                    "View Game"
                </A>
                <Show when=move || game.round_state == "waiting">
                    <button class="button button-small button-primary">
                        "Join Game"
                    </button>
                </Show>
                <Show when=move || game.round_state == "active">
                    <A href=format!("/games/{}/spectate", game.id) class="button button-small">
                        "Spectate"
                    </A>
                </Show>
            </div>
        </div>
    }
}

#[component]
pub fn CreateGamePage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let navigate = leptos_router::hooks::use_navigate();
    
    let (time_control, set_time_control) = create_signal("none".to_string());
    let (creating, set_creating) = create_signal(false);
    let (error, set_error) = create_signal(Option::<String>::None);

    let create_game = move |_| {
        set_creating(true);
        set_error(None);
        
        let time_control_value = if time_control.get() == "none" {
            None
        } else {
            Some(time_control.get())
        };
        
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.create_game(time_control_value).await {
                Ok(game) => {
                    navigate(&format!("/games/{}", game.id), Default::default());
                }
                Err(e) => {
                    set_error(Some(e));
                    set_creating(false);
                }
            }
        });
    };

    view! {
        <div class="create-game-page">
            <div class="create-game-card">
                <h1>"Create New Game"</h1>
                
                <div class="game-options">
                    <div class="option-group">
                        <label>"Time Control"</label>
                        <select
                            class="form-select"
                            on:change=move |ev| set_time_control(event_target_value(&ev))
                            prop:value=time_control
                        >
                            <option value="none">"No time limit"</option>
                            <option value="blitz">"Blitz (5 minutes)"</option>
                            <option value="rapid">"Rapid (10 minutes)"</option>
                            <option value="classical">"Classical (30 minutes)"</option>
                        </select>
                    </div>
                    
                    <div class="option-info">
                        <h3>"Game Rules"</h3>
                        <ul>
                            <li>"Board size: 11x11"</li>
                            <li>"Each player controls attractors and repulsors"</li>
                            <li>"Guide the automaton to your goal to win"</li>
                            <li>"First player to reach their goal wins"</li>
                        </ul>
                    </div>
                </div>
                
                <Show when=move || error.get().is_some()>
                    <div class="error-message">
                        {move || error.get().unwrap_or_default()}
                    </div>
                </Show>
                
                <div class="form-actions">
                    <button
                        class="button button-primary"
                        on:click=create_game
                        disabled=creating
                    >
                        {move || if creating.get() { "Creating..." } else { "Create Game" }}
                    </button>
                    <A href="/games" class="button button-secondary">
                        "Cancel"
                    </A>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn GamePage() -> impl IntoView {
    let params = use_params_map();
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let game_id = move || {
        params.get()
            .get("id")
            .and_then(|id| Uuid::parse_str(id).ok())
    };
    
    let (game_state, set_game_state) = create_signal(Option::<GameStateResponse>::None);
    let (selected_cell, set_selected_cell) = create_signal(Option::<(u8, u8)>::None);
    let (error, set_error) = create_signal(Option::<String>::None);
    let (loading, set_loading) = create_signal(true);
    
    // WebSocket connection
    let ws_connection = store_value(None);
    
    // Fetch initial game state and setup WebSocket
    Effect::new(move |_| {
        if let Some(id) = game_id() {
            spawn_local(async move {
                let client = ApiClient::new(
                    app_state.api_base_url.clone(),
                    app_state.auth_token.get()
                );
                
                match client.get_game(id).await {
                    Ok(game) => {
                        set_game_state(Some(game));
                        set_loading(false);
                        
                        // Setup WebSocket connection
                        if let Some(ws) = create_websocket_connection(&app_state) {
                            let _ = ws.subscribe_to_game(id).await;
                            ws_connection.set_value(Some(ws));
                        }
                    }
                    Err(e) => {
                        set_error(Some(e));
                        set_loading(false);
                    }
                }
            });
        }
    });
    
    view! {
        <div class="game-page">
            <Show
                when=move || loading.get()
                fallback=move || view! {
                    <Show
                        when=move || game_state.get().is_some()
                        fallback=move || view! {
                            <div class="error-state">
                                <h2>"Error loading game"</h2>
                                <p>{move || error.get().unwrap_or_else(|| "Game not found".to_string())}</p>
                                <A href="/games" class="button">"Back to Games"</A>
                            </div>
                        }
                    >
                        {move || {
                            let game = game_state.get().unwrap();
                            let board_signal = create_memo(move |_| game_state.get().unwrap().board);
                            
                            view! {
                                <div class="game-container">
                                    <div class="game-header">
                                        <h1>"Game " {game.id.to_string().chars().take(8).collect::<String>()}</h1>
                                        <div class="game-actions">
                                            <A href=format!("/games/{}/history", game.id) class="button button-small">
                                                "View History"
                                            </A>
                                        </div>
                                    </div>
                                    
                                    <div class="game-content">
                                        <div class="game-left-panel">
                                            <GameInfo game_state=game_state />
                                            <MoveControls
                                                game_state=game_state
                                                selected_cell=selected_cell
                                                on_submit_move=move |from_x, from_y, to_x, to_y| {
                                                    // Submit move logic
                                                }
                                            />
                                        </div>
                                        
                                        <div class="game-center">
                                            <GameBoard
                                                board=board_signal
                                                selected_cell=set_selected_cell
                                                on_cell_click=move |x, y| {
                                                    set_selected_cell.update(|sel| {
                                                        if *sel == Some((x, y)) {
                                                            *sel = None;
                                                        } else {
                                                            *sel = Some((x, y));
                                                        }
                                                    });
                                                }
                                                show_coordinates=create_memo(move |_| false)
                                                highlight_goals=create_memo(move |_| true)
                                            />
                                        </div>
                                        
                                        <div class="game-right-panel">
                                            <ChatPanel game_id=game.id />
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
                    <p>"Loading game..."</p>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn GameHistoryPage() -> impl IntoView {
    view! {
        <div class="game-history-page">
            <h1>"Game History"</h1>
            <p>"Game history view coming soon..."</p>
        </div>
    }
}

#[component]
pub fn SpectatePage() -> impl IntoView {
    view! {
        <div class="spectate-page">
            <h1>"Spectate Game"</h1>
            <p>"Spectator mode coming soon..."</p>
        </div>
    }
}
