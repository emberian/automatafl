// GameBoard component - renders the Automatafl board with traditional hnefetafl pieces
use crate::{helpers::create_api_client, state::AppState};
use automatafl_api_types::GameStateResponse;
use automatafl_logic::{Coord, Particle};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn GameBoard(
    game_id: Uuid,
    game_state: GameStateResponse,
    #[prop(optional)] on_move: Option<Callback<(Coord, Coord)>>,
) -> impl IntoView {
    let board = game_state.game.board;
    let app_state = use_context::<AppState>().expect("AppState should be provided");

    // Single selection state for clean click-to-move
    let (selected_cell, set_selected_cell) = signal(Option::<Coord>::None);

    // Optimistic move tracking (show move immediately before server confirms)
    let (optimistic_move, set_optimistic_move) = signal(Option::<(Coord, Coord)>::None);

    // Derived state from app_state
    let current_player_id = app_state.current_player_id.get();
    let my_pid = current_player_id.and_then(|id| game_state.player_ids.get(&id).copied());
    let has_pending_move = my_pid
        .map(|pid| game_state.game.pending_moves.iter().any(|mv| mv.who == pid))
        .unwrap_or(false);
    let can_interact = my_pid.is_some() && !has_pending_move;

    // Move submission action
    let move_action = Action::new_local(move |(gid, from, to): &(Uuid, Coord, Coord)| {
        let (gid, from, to) = (*gid, *from, *to);
        async move {
            let client = create_api_client();
            client.perform_move(gid, from, to).await
        }
    });

    // Clean click handler with optimistic update
    let handle_cell_click = move |coord: Coord| {
        if !can_interact {
            return;
        }

        match selected_cell.get() {
            None => set_selected_cell.set(Some(coord)),
            Some(from) if from == coord => set_selected_cell.set(None),
            Some(from) => {
                // Optimistically show the move
                set_optimistic_move.set(Some((from, coord)));

                if let Some(callback) = on_move {
                    callback.run((from, coord));
                } else {
                    move_action.dispatch((game_id, from, coord));
                }
                set_selected_cell.set(None);

                // Clear optimistic move after a delay if not confirmed
                gloo_timers::callback::Timeout::new(5000, move || {
                    set_optimistic_move.set(None);
                })
                .forget();
            }
        }
    };

    // Clear optimistic move on server response
    Effect::new(move |_| {
        if move_action.value().get().is_some() {
            set_optimistic_move.set(None);
        }
    });

    // Get visual indicators from app state
    let app_state_for_move_events = app_state.clone();
    let move_events = Memo::new(move |_| {
        let events = app_state_for_move_events.get_move_events(game_id);
        let now = js_sys::Date::now() as u64;
        events
            .into_iter()
            .filter(|e| now - e.timestamp < 3000)
            .collect::<Vec<_>>()
    });

    let app_state_for_conflict_events = app_state.clone();
    let conflict_events = Memo::new(move |_| {
        let events = app_state_for_conflict_events.get_conflict_events(game_id);
        let now = js_sys::Date::now() as u64;
        events
            .into_iter()
            .filter(|e| now - e.timestamp < 2000)
            .collect::<Vec<_>>()
    });

    view! {
        <div class="game-board-container">
            // SVG-based board with traditional piece rendering
            <svg
                class="game-board-svg"
                viewBox=format!("0 0 {} {}", board.size.x, board.size.y)
                style=format!("width: {}em; height: {}em;", board.size.x as f32 * 3.5, board.size.y as f32 * 3.5)
            >
                <defs>
                    <PieceDefs />
                    <marker id="arrowhead" markerWidth="10" markerHeight="7"
                            refX="9" refY="3.5" orient="auto">
                        <polygon points="0 0, 10 3.5, 0 7" fill="currentColor" />
                    </marker>
                </defs>

                // Board cells and pieces
                {(0..board.size.y).map(|y| {
                    (0..board.size.x).map(|x| {
                        let coord = Coord { x, y };
                        let cell = board.particles[(x as usize, y as usize)];
                        let is_automaton = board.automaton_location == coord;
                        let is_goal = game_state.game.goals.iter().any(|(c, _)| *c == coord);
                        let goal_player = game_state.game.goals.iter()
                            .find(|(c, _)| *c == coord)
                            .map(|(_, pid)| pid.0);
                        let is_selected = selected_cell.get() == Some(coord);

                        view! {
                            <BoardCell
                                coord=coord
                                particle=cell.what
                                is_automaton=is_automaton
                                is_goal=is_goal
                                goal_player=goal_player
                                is_selected=is_selected
                                can_interact=can_interact
                                on_click=handle_cell_click
                            />
                        }
                    }).collect::<Vec<_>>()
                }).collect::<Vec<_>>()}

                // Optimistic move indicator
                {move || {
                    optimistic_move.get().map(|(from, to)| {
                        view! {
                            <MoveArrow
                                from=from
                                to=to
                                color="#888"
                                class="optimistic-move"
                            />
                        }
                    })
                }}

                // Confirmed move indicators
                {move || {
                    move_events.get().iter().map(|event| {
                        let success_color = if event.success { "#070" } else { "#f00" };
                        view! {
                            <MoveArrow
                                from=event.from
                                to=event.to
                                color=success_color
                            />
                        }
                    }).collect_view()
                }}

                // Conflict markers
                {move || {
                    conflict_events.get().iter().map(|event| {
                        view! {
                            <circle
                                cx=event.coord.x as f32 + 0.5
                                cy=event.coord.y as f32 + 0.5
                                r="0.45"
                                fill="rgba(200, 0, 0, 0.6)"
                                class="conflict-pulse"
                            />
                        }
                    }).collect_view()
                }}
            </svg>

            // Status display
            <BoardStatus
                has_pending_move=has_pending_move
                can_interact=can_interact
                my_pid=my_pid.map(|pid| pid.0)
                selected_cell=selected_cell.get()
                on_cancel=move || set_selected_cell.set(None)
                move_result=move_action.value().get()
            />
        </div>
    }
}

// SVG piece definitions - traditional hnefetafl style
#[component]
fn PieceDefs() -> impl IntoView {
    view! {
        <g id="piece-attractor">
            <circle cx="0.5" cy="0.5" r="0.42" fill="#ddd" stroke="#777" stroke-width="0.04"/>
            <circle cx="0.35" cy="0.35" r="0.08" fill="#fff"/>
            <text x="0.5" y="0.5" text-anchor="middle" dominant-baseline="central"
                  font-size="0.5" fill="#333" font-weight="bold">"⊕"</text>
        </g>
        <g id="piece-repulsor">
            <circle cx="0.5" cy="0.5" r="0.42" fill="#222" stroke="#000" stroke-width="0.04"/>
            <circle cx="0.35" cy="0.35" r="0.08" fill="#777"/>
            <text x="0.5" y="0.5" text-anchor="middle" dominant-baseline="central"
                  font-size="0.5" fill="#ddd" font-weight="bold">"⊖"</text>
        </g>
        <g id="piece-automaton">
            <circle cx="0.5" cy="0.5" r="0.42" fill="#990" stroke="#550" stroke-width="0.04"/>
            <circle cx="0.35" cy="0.35" r="0.08" fill="#bb6"/>
            <text x="0.5" y="0.5" text-anchor="middle" dominant-baseline="central"
                  font-size="0.5" fill="#ffea00" font-weight="bold">"◉"</text>
        </g>
    }
}

// Individual board cell component
#[component]
fn BoardCell(
    coord: Coord,
    particle: Particle,
    is_automaton: bool,
    is_goal: bool,
    goal_player: Option<u8>,
    is_selected: bool,
    can_interact: bool,
    on_click: impl Fn(Coord) + 'static + Copy,
) -> impl IntoView {
    let x = coord.x as f32;
    let y = coord.y as f32;

    let piece_id = if is_automaton {
        "#piece-automaton"
    } else {
        match particle {
            Particle::Attractor => "#piece-attractor",
            Particle::Repulsor => "#piece-repulsor",
            Particle::Automaton => "#piece-automaton",
            Particle::Vacuum => "",
        }
    };

    let cell_fill = if is_goal {
        match goal_player {
            Some(1) => "#e8d4a0",
            Some(2) => "#a0c4e8",
            Some(3) => "#e8a0a0",
            Some(4) => "#a0e8a0",
            _ => "#ca8",
        }
    } else {
        "#ca8"
    };

    view! {
        <g>
            <rect
                x=x y=y width="1" height="1"
                fill=cell_fill
                stroke="#000"
                stroke-width="0.03"
                class=move || if can_interact { "cell-interactive" } else { "" }
                on:click=move |_| on_click(coord)
            />
            {if is_selected {
                view! {
                    <rect
                        x=x y=y width="1" height="1"
                        fill="rgba(0, 100, 0, 0.3)"
                        stroke="#0a0"
                        stroke-width="0.06"
                        class="selection-indicator"
                    />
                }.into_any()
            } else {
                view! {}.into_any()
            }}
            {if !piece_id.is_empty() {
                view! {
                    <use_ href=piece_id x=x y=y width="1" height="1" />
                }.into_any()
            } else {
                view! {}.into_any()
            }}
        </g>
    }
}

// Status display component
#[component]
fn BoardStatus(
    has_pending_move: bool,
    can_interact: bool,
    my_pid: Option<u8>,
    selected_cell: Option<Coord>,
    on_cancel: impl Fn() + 'static + Copy,
    move_result: Option<
        Result<automatafl_api_types::MoveResultResponse, automatafl_backend_client::ClientError>,
    >,
) -> impl IntoView {
    view! {
        <div class="board-status">
            {if has_pending_move {
                view! { <p class="status-info">"✓ Move submitted! Waiting for other players..."</p> }.into_any()
            } else if can_interact {
                match selected_cell {
                    Some(coord) => view! {
                        <div class="status-controls">
                            <p>"Selected: (" {coord.x} ", " {coord.y} ") - Click destination"</p>
                            <button class="button button-small" on:click=move |_| on_cancel()>"Cancel"</button>
                        </div>
                    }.into_any(),
                    None => view! { <p>"Click a piece to select, then click where to move"</p> }.into_any()
                }
            } else if my_pid.is_some() {
                view! { <p>"You've already submitted your move"</p> }.into_any()
            } else {
                view! { <p class="status-spectator">"Spectating"</p> }.into_any()
            }}

            {match move_result {
                Some(Ok(_)) => view! { <p class="status-success">"Move submitted!"</p> }.into_any(),
                Some(Err(e)) => view! { <p class="status-error">{format!("Error: {}", e)}</p> }.into_any(),
                None => view! {}.into_any()
            }}
        </div>
    }
}

// Curved move arrow - similar to original game.html
#[component]
fn MoveArrow(
    from: Coord,
    to: Coord,
    color: &'static str,
    #[prop(optional)] class: &'static str,
) -> impl IntoView {
    let from_x = from.x as f32 + 0.5;
    let from_y = from.y as f32 + 0.5;
    let to_x = to.x as f32 + 0.5;
    let to_y = to.y as f32 + 0.5;

    // Curve control point (like original)
    let is_vertical = (from.y as i32 - to.y as i32).abs() > (from.x as i32 - to.x as i32).abs();
    let mid_x = (from_x + to_x) / 2.0;
    let mid_y = (from_y + to_y) / 2.0;

    let (ctrl_x, ctrl_y) = if is_vertical {
        (mid_x + 0.5, mid_y)
    } else {
        (mid_x, mid_y + 0.5)
    };

    let path_d = format!(
        "M {} {} Q {} {} {} {}",
        from_x, from_y, ctrl_x, ctrl_y, to_x, to_y
    );

    view! {
        <path
            d=path_d
            stroke=color
            stroke-width="0.1"
            fill="none"
            class=format!("move-path-animated {}", class)
        />
    }
}
