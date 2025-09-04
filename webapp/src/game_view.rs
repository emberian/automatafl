use crate::{
    api::ApiClient,
    auth::AuthState,
    board::{GameBoard, MoveControls, MoveSelection},
    chat::GameChat,
    websocket::WebSocketConnection,
};
use automatafl_api::{GameStateResponse, Position, SubmitMoveRequest, WebSocketMessage};
use leptos::prelude::*;
use leptos::task::spawn_local;
use uuid::Uuid;

#[component]
pub fn GameView(
    game_id: Uuid,
    api: ApiClient,
    auth: AuthState,
    ws: WebSocketConnection,
    on_leave_game: impl Fn() + 'static,
) -> impl IntoView {
    let (game_state, set_game_state) = create_signal(None::<GameStateResponse>);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(None::<String>);
    let (move_selection, set_move_selection) = create_signal(MoveSelection {
        from: None,
        to: None,
    });
    let (active_tab, set_active_tab) = create_signal("game");

    // Load initial game state
    let api_clone = api.clone();
    create_effect(move |_| {
        spawn_local(async move {
            set_loading.set(true);
            match api_clone.get_game(game_id).await {
                Ok(game) => {
                    set_game_state.set(Some(game));
                    set_error.set(None);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            set_loading.set(false);
        });
    });

    // Subscribe to WebSocket updates
    let ws_clone = ws.clone();
    create_effect(move |_| {
        spawn_local(async move {
            let _ = ws_clone.subscribe_to_game(game_id).await;
        });
        
        on_cleanup(move || {
            let ws = ws.clone();
            spawn_local(async move {
                let _ = ws.unsubscribe_from_game(game_id).await;
            });
        });
    });

    // Handle WebSocket messages
    let ws_msg = use_context::<ReadSignal<Option<WebSocketMessage>>>();
    if let Some(ws_msg) = ws_msg {
        create_effect(move |_| {
            if let Some(msg) = ws_msg.get() {
                match msg {
                    WebSocketMessage::GameUpdate { game_state: new_state } => {
                        if new_state.id == game_id {
                            set_game_state.set(Some(new_state));
                        }
                    }
                    WebSocketMessage::Error { error } => {
                        set_error.set(Some(error));
                    }
                    _ => {}
                }
            }
        });
    }

    let handle_cell_click = move |x: u8, y: u8| {
        let mut selection = move_selection.get();
        
        if selection.from.is_none() {
            // First click - select source
            selection.from = Some(Position { x, y });
            selection.to = None;
        } else if selection.to.is_none() {
            // Second click - select destination
            if selection.from.as_ref().map(|p| p.x == x && p.y == y).unwrap_or(false) {
                // Clicked same cell - deselect
                selection.from = None;
            } else {
                selection.to = Some(Position { x, y });
            }
        } else {
            // Already have both - start new selection
            selection.from = Some(Position { x, y });
            selection.to = None;
        }
        
        set_move_selection.set(selection);
    };

    let submit_move = move |from: Position, to: Position| {
        let api = api.clone();
        spawn_local(async move {
            let move_data = SubmitMoveRequest {
                from_x: from.x,
                from_y: from.y,
                to_x: to.x,
                to_y: to.y,
            };
            
            match api.submit_move(game_id, move_data).await {
                Ok(response) => {
                    set_game_state.set(Some(response.game_state));
                    leptos::logging::log!("Move feedback: {}", response.feedback);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
        });
    };

    let join_game = move |_| {
        let api = api.clone();
        spawn_local(async move {
            match api.join_game(game_id).await {
                Ok(game) => {
                    set_game_state.set(Some(game));
                    set_error.set(None);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
        });
    };

    let current_user_id = auth.user.get().map(|u| u.id);

    view! {
        <div class="game-view">
            <div class="game-header">
                <h2>"Game " {game_id.to_string()[..8].to_string()}</h2>
                <button class="btn btn-secondary" on:click=move |_| on_leave_game()>
                    "Leave Game"
                </button>
            </div>

            {move || error.get().map(|e| view! {
                <div class="alert alert-error">{e}</div>
            })}

            {move || if loading.get() {
                view! {
                    <div class="loading">
                        <p>"Loading game..."</p>
                    </div>
                }.into_view()
            } else if let Some(game) = game_state.get() {
                let user_is_player = current_user_id.map(|id| {
                    game.white_player.as_ref().map(|p| p.id == id).unwrap_or(false) ||
                    game.black_player.as_ref().map(|p| p.id == id).unwrap_or(false)
                }).unwrap_or(false);
                
                let can_join = !user_is_player && 
                    (game.white_player.is_none() || game.black_player.is_none()) &&
                    game.winner.is_none();

                view! {
                    <div class="game-content">
                        <div class="tab-container">
                            <button
                                class={move || if active_tab.get() == "game" { "tab active" } else { "tab" }}
                                on:click=move |_| set_active_tab.set("game")
                            >
                                "Game"
                            </button>
                            <button
                                class={move || if active_tab.get() == "chat" { "tab active" } else { "tab" }}
                                on:click=move |_| set_active_tab.set("chat")
                            >
                                "Chat"
                            </button>
                        </div>

                        {move || match active_tab.get() {
                            "game" => view! {
                                <div class="game-tab">
                                    {game.winner.map(|winner_id| {
                                        let winner_name = game.white_player.as_ref()
                                            .filter(|p| p.id == winner_id)
                                            .or(game.black_player.as_ref().filter(|p| p.id == winner_id))
                                            .map(|p| p.username.clone())
                                            .unwrap_or_else(|| "Unknown".to_string());
                                        
                                        view! {
                                            <div class="winner">
                                                "🎉 Winner: " {winner_name} " 🎉"
                                            </div>
                                        }
                                    })}
                                    
                                    <div class="game-area">
                                        <div class="game-status">
                                            <h3>"Players"</h3>
                                            <p>"Player 1: " {game.white_player.as_ref()
                                                .map(|p| p.username.clone())
                                                .unwrap_or_else(|| "[Empty]".to_string())}
                                            </p>
                                            <p>"Player 2: " {game.black_player.as_ref()
                                                .map(|p| p.username.clone())
                                                .unwrap_or_else(|| "[Empty]".to_string())}
                                            </p>
                                            <p>"Spectators: " {game.spectator_count}</p>
                                            <p>"Round State: " {&game.round_state}</p>
                                            
                                            {can_join.then(|| view! {
                                                <button class="btn btn-primary" on:click=join_game>
                                                    "Join Game"
                                                </button>
                                            })}
                                        </div>
                                        
                                        <GameBoard
                                            board=game.board.clone()
                                            on_cell_click=handle_cell_click
                                            move_selection=move_selection
                                        />
                                        
                                        {user_is_player.then(|| view! {
                                            <MoveControls
                                                move_selection=move_selection
                                                set_move_selection=set_move_selection
                                                on_submit_move=submit_move
                                            />
                                        })}
                                    </div>
                                </div>
                            }.into_view(),
                            "chat" => view! {
                                <GameChat
                                    game_id=game_id
                                    api=api.clone()
                                    current_user_id=current_user_id
                                />
                            }.into_view(),
                            _ => view! { <div /> }.into_view(),
                        }}
                    </div>
                }.into_view()
            } else {
                view! {
                    <div class="error">
                        <p>"Failed to load game"</p>
                    </div>
                }.into_view()
            }}
        </div>
    }
}
