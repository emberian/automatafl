// GameBoard component - renders the Automatafl board
use crate::{api::ApiClient, state::AppState};
use automatafl_api_types::GameStateResponse;
use automatafl_logic::{Particle, Coord};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn GameBoard(
    game_id: Uuid, 
    game_state: GameStateResponse,
    #[prop(optional)] on_move: Option<Callback<(Coord, Coord)>>
) -> impl IntoView {
    let board = game_state.game.board;
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    // Selection state for click-to-move interaction
    let (selected_cell, set_selected_cell) = signal(Option::<Coord>::None);
    
    // Track recent moves for visualization
    let (recent_moves, _set_recent_moves) = signal(Vec::<(Coord, Coord, bool)>::new());
    
    // Track move animations and indicators
    let (move_indicators, set_move_indicators) = signal(Vec::<MoveIndicator>::new());
    
    // Track conflict markers
    let (conflict_markers, set_conflict_markers) = signal(Vec::<ConflictMarker>::new());
    
    // Track automaton move indicator
    let (automaton_move, _set_automaton_move) = signal(Option::<(Coord, Coord)>::None);
    
    // Check if current player can make moves
    let current_player_id = app_state.current_player_id.get();
    let my_pid = current_player_id.and_then(|id| game_state.player_ids.get(&id).copied());
    
    // Check if player has already submitted a move this round
    let has_pending_move = my_pid.map(|pid| {
        game_state.game.pending_moves.iter().any(|mv| mv.who == pid)
    }).unwrap_or(false);
    
    let can_interact = my_pid.is_some() && !has_pending_move;
    
    let api_base_url = app_state.api_base_url.clone();
    let move_action = Action::new_local(move |(gid, from, to): &(Uuid, Coord, Coord)| {
        let gid = *gid;
        let from = *from;
        let to = *to;
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.perform_move(gid, from, to).await
        }
    });
    
    let handle_cell_click = move |coord: Coord| {
        if !can_interact {
            return;
        }
        
        match selected_cell.get() {
            None => {
                // First click - select source
                set_selected_cell.set(Some(coord));
            }
            Some(from) => {
                if from == coord {
                    // Clicking same cell - deselect
                    set_selected_cell.set(None);
                } else {
                    // Second click - make move
                    if let Some(callback) = on_move {
                        callback.run((from, coord));
                    } else {
                        // Default behavior - submit move via API
                        move_action.dispatch((game_id, from, coord));
                    }
                    set_selected_cell.set(None);
                }
            }
        }
    };
    
    // Effect to handle move results and update visualization
    Effect::new(move |_| {
        if let Some(result) = move_action.value().get() {
            match result {
                Ok(_) => {
                    // Move was successful - visualization will be updated via WebSocket events
                }
                Err(_) => {
                    // Handle error case - could add visual feedback here
                }
            }
        }
    });
    
    // Listen for WebSocket events to update visualizations
    let app_state_for_events = app_state.clone();
    Effect::new(move |_| {
        // Get move events from app state and convert to indicators
        let move_events = app_state_for_events.get_move_events(game_id);
        let current_time = js_sys::Date::now() as u64;
        
        let indicators: Vec<MoveIndicator> = move_events.into_iter()
            .filter(|event| current_time - event.timestamp < 3000) // Show for 3 seconds
            .map(|event| MoveIndicator {
                from: event.from,
                to: event.to,
                player_id: event.player_id,
                success: event.success,
                timestamp: event.timestamp,
            })
            .collect();
        
        set_move_indicators.set(indicators);
        
        // Get conflict events from app state and convert to markers
        let conflict_events = app_state_for_events.get_conflict_events(game_id);
        let markers: Vec<ConflictMarker> = conflict_events.into_iter()
            .filter(|event| current_time - event.timestamp < 2000) // Show for 2 seconds
            .map(|event| ConflictMarker {
                coord: event.coord,
                players: event.players.clone(),
                timestamp: event.timestamp,
            })
            .collect();
        
        set_conflict_markers.set(markers);
    });
    
    view! {
        <div class="game-board" style="position: relative;">
            <div class="board-grid" style=format!("grid-template-columns: repeat({}, 1fr)", board.size.x)>
                {(0..board.size.y).map(|y| {
                    (0..board.size.x).map(|x| {
                        let coord = automatafl_logic::Coord { x, y };
                        let cell = board.particles[(x as usize, y as usize)];
                        let is_automaton = board.automaton_location == coord;
                        
                        // Check if this is a goal
                        let is_goal = game_state.game.goals.iter().any(|(c, _)| *c == coord);
                        let goal_player = game_state.game.goals.iter()
                            .find(|(c, _)| *c == coord)
                            .map(|(_, pid)| pid.0);
                        
                        // Check if this cell is selected
                        let is_selected = selected_cell.get() == Some(coord);
                        
                        let cell_class = format!(
                            "board-cell {} {} {} {} {}",
                            particle_class(cell.what),
                            if is_automaton { "automaton" } else { "" },
                            if is_goal { format!("goal goal-p{}", goal_player.unwrap_or(0)) } else { String::new() },
                            if is_selected { "selected" } else { "" },
                            if can_interact { "interactive" } else { "" }
                        );
                        
                        let coord_for_click = coord;
                        view! {
                            <div 
                                class=cell_class
                                data-x=x
                                data-y=y
                                on:click=move |_| handle_cell_click(coord_for_click)
                            >
                                <span class="cell-content">
                                    {particle_symbol(cell.what, is_automaton)}
                                </span>
                                {if is_selected {
                                    view! { <div class="selection-pulse"></div> }.into_any()
                                } else {
                                    view! {}.into_any()
                                }}
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }).collect::<Vec<_>>()}
            </div>
            
            // Move indicators overlay
            <div class="move-indicators-overlay">
                {move || {
                    move_indicators.get().into_iter().map(|indicator| {
                        let player_class = format!("player-{}", indicator.player_id);
                        let success_class = if indicator.success { "success" } else { "failed" };
                        
                        view! {
                            <MoveArrow 
                                from=indicator.from 
                                to=indicator.to 
                                board_size=(board.size.x, board.size.y)
                                class=format!("move-arrow {} {}", player_class, success_class)
                            />
                        }
                    }).collect_view()
                }}
            </div>
            
            // Conflict markers overlay
            <div class="conflict-markers-overlay">
                {move || {
                    conflict_markers.get().into_iter().map(|marker| {
                        view! {
                            <ConflictMarkerView 
                                coord=marker.coord 
                                board_size=(board.size.x, board.size.y)
                                players=marker.players.clone()
                            />
                        }
                    }).collect_view()
                }}
            </div>
            
            // Automaton move indicator
            {move || {
                if let Some((from, to)) = automaton_move.get() {
                    view! {
                        <div class="automaton-move-overlay">
                            <MoveArrow 
                                from=from 
                                to=to 
                                board_size=(board.size.x, board.size.y)
                                class="automaton-move".to_string()
                            />
                        </div>
                    }.into_any()
                } else {
                    view! {}.into_any()
                }
            }}
            
            {move || {
                if has_pending_move {
                    view! {
                        <div class="board-status">
                            <p>"✓ Move submitted! Waiting for other players..."</p>
                        </div>
                    }.into_any()
                } else if can_interact {
                    match selected_cell.get() {
                        Some(coord) => view! {
                            <div class="board-status">
                                <p>"Selected: (" {coord.x} ", " {coord.y} ") - Click another cell to move"</p>
                                <button 
                                    class="button button-small"
                                    on:click=move |_| set_selected_cell.set(None)
                                >
                                    "Cancel Selection"
                                </button>
                            </div>
                        }.into_any(),
                        None => view! {
                            <div class="board-status">
                                <p>"Click a cell to select it, then click destination to move"</p>
                            </div>
                        }.into_any()
                    }
                } else if my_pid.is_some() {
                    view! {
                        <div class="board-status">
                            <p>"Waiting to submit your move..."</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="board-status">
                            <p>"You are spectating this game"</p>
                        </div>
                    }.into_any()
                }
            }}
            
            {move || {
                if let Some(result) = move_action.value().get() {
                    match result {
                        Ok(_) => view! {
                            <div class="move-feedback success">
                                "Move submitted successfully!"
                            </div>
                        }.into_any(),
                        Err(e) => view! {
                            <div class="move-feedback error">
                                "Error: " {format!("{}", e)}
                            </div>
                        }.into_any()
                    }
                } else {
                    view! {}.into_any()
                }
            }}
        </div>
    }
}

fn particle_class(particle: Particle) -> &'static str {
    match particle {
        Particle::Attractor => "attractor",
        Particle::Repulsor => "repulsor",
        Particle::Automaton => "automaton",
        Particle::Vacuum => "vacuum",
    }
}

fn particle_symbol(particle: Particle, is_automaton: bool) -> &'static str {
    if is_automaton {
        "◉"
    } else {
        match particle {
            Particle::Attractor => "⊕",
            Particle::Repulsor => "⊖",
            Particle::Automaton => "◉",
            Particle::Vacuum => "",
        }
    }
}

#[derive(Clone, Debug)]
struct MoveIndicator {
    from: Coord,
    to: Coord,
    player_id: u8,
    success: bool,
    timestamp: u64,
}

impl MoveIndicator {
    fn new(from: Coord, to: Coord, player_id: u8, success: bool) -> Self {
        Self {
            from,
            to,
            player_id,
            success,
            timestamp: js_sys::Date::now() as u64,
        }
    }
    
    fn is_expired(&self, current_time: u64) -> bool {
        current_time - self.timestamp > 3000 // 3 seconds
    }
}

#[derive(Clone, Debug)]
struct ConflictMarker {
    coord: Coord,
    players: Vec<u8>,
    timestamp: u64,
}

impl ConflictMarker {
    fn new(coord: Coord, players: Vec<u8>) -> Self {
        Self {
            coord,
            players,
            timestamp: js_sys::Date::now() as u64,
        }
    }
    
    fn is_expired(&self, current_time: u64) -> bool {
        current_time - self.timestamp > 2000 // 2 seconds
    }
}

#[component]
fn MoveArrow(
    from: Coord,
    to: Coord,
    board_size: (u8, u8),
    #[prop(optional)] class: Option<String>
) -> impl IntoView {
    let (board_width, board_height) = board_size;
    
    // Calculate positions as percentages
    let from_x = (from.x as f32 + 0.5) / board_width as f32 * 100.0;
    let from_y = (from.y as f32 + 0.5) / board_height as f32 * 100.0;
    let to_x = (to.x as f32 + 0.5) / board_width as f32 * 100.0;
    let to_y = (to.y as f32 + 0.5) / board_height as f32 * 100.0;
    
    // Calculate curve control point (similar to original game.html)
    let is_vertical = (from.y as i32 - to.y as i32).abs() > (from.x as i32 - to.x as i32).abs();
    let (ctrl_x, ctrl_y) = if is_vertical {
        ((from_x + to_x) / 2.0 + 5.0, (from_y + to_y) / 2.0)
    } else {
        ((from_x + to_x) / 2.0, (from_y + to_y) / 2.0 + 5.0)
    };
    
    let path_d = format!(
        "M {},{} Q {},{} {},{}",
        from_x, from_y, ctrl_x, ctrl_y, to_x, to_y
    );
    
    let arrow_class = class.unwrap_or_default();
    
    view! {
        <svg class=format!("move-arrow-svg {}", arrow_class) viewBox="0 0 100 100">
            <defs>
                <marker id="arrowhead" markerWidth="10" markerHeight="7" 
                        refX="9" refY="3.5" orient="auto">
                    <polygon points="0 0, 10 3.5, 0 7" fill="currentColor" />
                </marker>
            </defs>
            <path 
                d=path_d
                stroke="currentColor" 
                stroke-width="2" 
                fill="none" 
                marker-end="url(#arrowhead)"
            />
        </svg>
    }
}

#[component]
fn ConflictMarkerView(
    coord: Coord,
    board_size: (u8, u8),
    players: Vec<u8>
) -> impl IntoView {
    let (board_width, board_height) = board_size;
    
    // Calculate position as percentages
    let x = (coord.x as f32 + 0.5) / board_width as f32 * 100.0;
    let y = (coord.y as f32 + 0.5) / board_height as f32 * 100.0;
    
    let players_text = players.iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    
    view! {
        <div 
            class="conflict-marker"
            style=format!("left: {}%; top: {}%;", x, y)
            title=format!("Conflict: Players {}", players_text)
        >
            <div class="conflict-pulse"></div>
            <span class="conflict-icon">"⚠"</span>
        </div>
    }
}
