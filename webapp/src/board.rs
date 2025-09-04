use automatafl_api::{BoardState, CellState, Position};
use leptos::prelude::*;

const CELL_SIZE: f32 = 50.0;
const BOARD_PADDING: f32 = 20.0;

#[derive(Clone)]
pub struct MoveSelection {
    pub from: Option<Position>,
    pub to: Option<Position>,
}

#[component]
pub fn GameBoard(
    board: BoardState,
    on_cell_click: impl Fn(u8, u8) + Clone + 'static,
    move_selection: ReadSignal<MoveSelection>,
) -> impl IntoView {
    let width = board.width as f32 * CELL_SIZE + 2.0 * BOARD_PADDING;
    let height = board.height as f32 * CELL_SIZE + 2.0 * BOARD_PADDING;
    let viewbox = format!("0 0 {} {}", width, height);

    view! {
        <div class="board-container">
            <svg
                width=format!("{}px", width)
                height=format!("{}px", height)
                viewBox=viewbox
                class="game-board"
            >
                <BoardDefs />
                <BoardGrid board=board.clone() on_cell_click move_selection />
                <BoardPieces board=board.clone() />
                <MoveIndicators move_selection />
            </svg>
        </div>
    }
}

#[component]
fn BoardDefs() -> impl IntoView {
    view! {
        <defs>
            // Vacuum (empty) cell
            <g id="vacuum"></g>
            
            // Repulsor piece
            <g id="repulsor">
                <circle
                    cx="25"
                    cy="25"
                    r="20"
                    style="stroke: #000; stroke-width: 2; fill: #222"
                />
                <circle
                    cx="20"
                    cy="20"
                    r="5"
                    style="fill: #777"
                />
            </g>
            
            // Attractor piece
            <g id="attractor">
                <circle
                    cx="25"
                    cy="25"
                    r="20"
                    style="stroke: #777; stroke-width: 2; fill: #ddd"
                />
                <circle
                    cx="20"
                    cy="20"
                    r="5"
                    style="fill: #fff"
                />
            </g>
            
            // Automaton piece
            <g id="automaton">
                <circle
                    cx="25"
                    cy="25"
                    r="20"
                    style="stroke: #550; stroke-width: 2; fill: #990"
                />
                <circle
                    cx="20"
                    cy="20"
                    r="5"
                    style="fill: #bb6"
                />
            </g>
            
            // Goal marker for corners
            <g id="goal-marker">
                <rect
                    x="5"
                    y="5"
                    width="40"
                    height="40"
                    style="stroke: #f90; stroke-width: 3; fill: none; stroke-dasharray: 5,5"
                />
            </g>
            
            // Selection marker
            <g id="selection-marker">
                <circle
                    cx="25"
                    cy="25"
                    r="22"
                    style="fill: rgba(0, 128, 0, 0.3); stroke: #080; stroke-width: 2"
                />
            </g>
            
            // Conflict marker
            <g id="conflict-marker">
                <circle
                    cx="25"
                    cy="25"
                    r="22"
                    style="fill: rgba(255, 0, 0, 0.3); stroke: #f00; stroke-width: 2"
                />
            </g>
        </defs>
    }
}

#[component]
fn BoardGrid(
    board: BoardState,
    on_cell_click: impl Fn(u8, u8) + Clone + 'static,
    move_selection: ReadSignal<MoveSelection>,
) -> impl IntoView {
    let cells = (0..board.height).flat_map(move |y| {
        (0..board.width).map(move |x| {
            let cell = board.cells[y as usize][x as usize].clone();
            let is_selected = move || {
                let selection = move_selection.get();
                selection.from.as_ref().map(|p| p.x == x && p.y == y).unwrap_or(false)
            };
            
            view! {
                <BoardCell
                    x=x
                    y=y
                    cell=cell
                    board_height=board.height
                    on_click=on_cell_click.clone()
                    is_selected=is_selected
                />
            }
        })
    }).collect_view();

    cells
}

#[component]
fn BoardCell(
    x: u8,
    y: u8,
    cell: CellState,
    board_height: u8,
    on_click: impl Fn(u8, u8) + 'static,
    is_selected: impl Fn() -> bool + 'static,
) -> impl IntoView {
    let px = x as f32 * CELL_SIZE + BOARD_PADDING;
    let py = (board_height - y - 1) as f32 * CELL_SIZE + BOARD_PADDING;
    
    let fill = if cell.is_goal {
        "#e8d4b0"
    } else {
        "#ca8"
    };

    view! {
        <g>
            <rect
                x=px
                y=py
                width=CELL_SIZE
                height=CELL_SIZE
                style=format!("stroke: #000; stroke-width: 1; fill: {}; cursor: pointer", fill)
                on:click=move |_| on_click(x, y)
            />
            {cell.is_goal.then(|| view! {
                <use
                    href="#goal-marker"
                    x=px
                    y=py
                />
            })}
            {move || is_selected().then(|| view! {
                <use
                    href="#selection-marker"
                    x=px
                    y=py
                />
            })}
            {cell.goal_player.map(|player| view! {
                <text
                    x=px + CELL_SIZE / 2.0
                    y=py + CELL_SIZE - 5.0
                    style="font-size: 10px; text-anchor: middle; fill: #666"
                >
                    {format!("P{}", player)}
                </text>
            })}
        </g>
    }
}

#[component]
fn BoardPieces(board: BoardState) -> impl IntoView {
    let pieces = (0..board.height).flat_map(move |y| {
        (0..board.width).map(move |x| {
            let cell = &board.cells[y as usize][x as usize];
            cell.particle.as_ref().map(|particle| {
                let px = x as f32 * CELL_SIZE + BOARD_PADDING;
                let py = (board.height - y - 1) as f32 * CELL_SIZE + BOARD_PADDING;
                let href = match particle.as_str() {
                    "repulsor" => "#repulsor",
                    "attractor" => "#attractor",
                    "automaton" => "#automaton",
                    _ => "#vacuum",
                };
                
                view! {
                    <use
                        href=href
                        x=px
                        y=py
                    />
                }
            })
        })
    }).collect_view();

    pieces
}

#[component]
fn MoveIndicators(move_selection: ReadSignal<MoveSelection>) -> impl IntoView {
    view! {
        {move || {
            let selection = move_selection.get();
            if let (Some(from), Some(to)) = (selection.from, selection.to) {
                let from_x = from.x as f32 * CELL_SIZE + BOARD_PADDING + CELL_SIZE / 2.0;
                let from_y = from.y as f32 * CELL_SIZE + BOARD_PADDING + CELL_SIZE / 2.0;
                let to_x = to.x as f32 * CELL_SIZE + BOARD_PADDING + CELL_SIZE / 2.0;
                let to_y = to.y as f32 * CELL_SIZE + BOARD_PADDING + CELL_SIZE / 2.0;
                
                Some(view! {
                    <path
                        d=format!("M {} {} L {} {}", from_x, from_y, to_x, to_y)
                        style="stroke: #080; stroke-width: 3; fill: none; marker-end: url(#arrowhead)"
                    />
                })
            } else {
                None
            }
        }}
    }
}

#[component]
pub fn MoveControls(
    move_selection: ReadSignal<MoveSelection>,
    set_move_selection: WriteSignal<MoveSelection>,
    on_submit_move: impl Fn(Position, Position) + 'static,
) -> impl IntoView {
    let can_submit = move || {
        let selection = move_selection.get();
        selection.from.is_some() && selection.to.is_some()
    };

    let clear_selection = move |_| {
        set_move_selection.set(MoveSelection {
            from: None,
            to: None,
        });
    };

    let submit_move = move |_| {
        let selection = move_selection.get();
        if let (Some(from), Some(to)) = (selection.from, selection.to) {
            on_submit_move(from, to);
            set_move_selection.set(MoveSelection {
                from: None,
                to: None,
            });
        }
    };

    view! {
        <div class="move-controls">
            <h3>"Move Selection"</h3>
            {move || {
                let selection = move_selection.get();
                if let Some(from) = selection.from {
                    view! {
                        <div class="selected-info">
                            <p>"From: " {format!("({}, {})", from.x, from.y)}</p>
                            {selection.to.map(|to| view! {
                                <p>"To: " {format!("({}, {})", to.x, to.y)}</p>
                            })}
                            <button
                                class="btn btn-secondary"
                                on:click=clear_selection
                            >
                                "Clear Selection"
                            </button>
                            {can_submit().then(|| view! {
                                <button
                                    class="btn btn-primary"
                                    on:click=submit_move
                                >
                                    "Submit Move"
                                </button>
                            })}
                        </div>
                    }
                } else {
                    view! {
                        <div class="move-hint">
                            <p>"Click a piece to select it, then click the destination square"</p>
                        </div>
                    }
                }
            }}
        </div>
    }
}