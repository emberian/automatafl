use automatafl_api::{BoardState, CellState, Position};
use leptos::prelude::*;
use leptos::svg;

#[component]
pub fn GameBoard(
    board: Signal<BoardState>,
    selected_cell: ReadSignal<Option<(u8, u8)>>,
    on_cell_click: impl Fn(u8, u8) + 'static + Copy,
    show_coordinates: Signal<bool>,
    highlight_goals: Signal<bool>,
) -> impl IntoView {
    let board_width = move || board.get().width as f32;
    let board_height = move || board.get().height as f32;
    let cell_size = 50.0;
    let svg_width = move || board_width() * cell_size;
    let svg_height = move || board_height() * cell_size;

    view! {
        <div class="board-container">
            <svg
                width=move || svg_width()
                height=move || svg_height()
                viewBox=move || format!("0 0 {} {}", svg_width(), svg_height())
                class="game-board-svg"
            >
                // Board cells
                {move || {
                    let board_state = board.get();
                    let selected = selected_cell.get();
                    
                    board_state.cells.iter().enumerate().flat_map(|(y, row)| {
                        row.iter().enumerate().map(move |(x, cell)| {
                            let x = x as u8;
                            let y = y as u8;
                            let is_selected = selected == Some((x, y));
                            let is_automaton = board_state.automaton_position.x == x && 
                                              board_state.automaton_position.y == y;
                            
                            view! {
                                <g>
                                    // Cell background
                                    <rect
                                        x=x as f32 * cell_size
                                        y=y as f32 * cell_size
                                        width=cell_size
                                        height=cell_size
                                        class="board-cell"
                                        class:selected=is_selected
                                        class:goal-p1=move || cell.is_goal && cell.goal_player == Some(1) && highlight_goals.get()
                                        class:goal-p2=move || cell.is_goal && cell.goal_player == Some(2) && highlight_goals.get()
                                        on:click=move |_| on_cell_click(x, y)
                                    />
                                    
                                    // Grid lines
                                    <rect
                                        x=x as f32 * cell_size
                                        y=y as f32 * cell_size
                                        width=cell_size
                                        height=cell_size
                                        fill="none"
                                        stroke="#333"
                                        stroke-width="1"
                                    />
                                    
                                    // Particle/content
                                    {move || render_cell_content(cell, x, y, cell_size, is_automaton)}
                                    
                                    // Coordinates (if enabled)
                                    <Show when=move || show_coordinates.get()>
                                        <text
                                            x=x as f32 * cell_size + cell_size * 0.5
                                            y=y as f32 * cell_size + cell_size * 0.9
                                            text-anchor="middle"
                                            font-size="10"
                                            fill="#666"
                                            pointer-events="none"
                                        >
                                            {format!("{},{}", x, y)}
                                        </text>
                                    </Show>
                                </g>
                            }
                        }).collect::<Vec<_>>()
                    }).collect::<Vec<_>>()
                }}
                
                // Goal markers
                {move || {
                    let board_state = board.get();
                    board_state.cells.iter().enumerate().flat_map(|(y, row)| {
                        row.iter().enumerate().filter_map(move |(x, cell)| {
                            if cell.is_goal && highlight_goals.get() {
                                let player = cell.goal_player?;
                                Some(view! {
                                    <g>
                                        // Goal star
                                        <path
                                            d=star_path(
                                                x as f32 * cell_size + cell_size * 0.5,
                                                y as f32 * cell_size + cell_size * 0.5,
                                                cell_size * 0.15
                                            )
                                            fill=if player == 1 { "#4CAF50" } else { "#2196F3" }
                                            opacity="0.7"
                                            pointer-events="none"
                                        />
                                        <text
                                            x=x as f32 * cell_size + cell_size * 0.5
                                            y=y as f32 * cell_size + cell_size * 0.2
                                            text-anchor="middle"
                                            font-size="12"
                                            font-weight="bold"
                                            fill=if player == 1 { "#4CAF50" } else { "#2196F3" }
                                            pointer-events="none"
                                        >
                                            {format!("P{}", player)}
                                        </text>
                                    </g>
                                })
                            } else {
                                None
                            }
                        }).collect::<Vec<_>>()
                    }).collect::<Vec<_>>()
                }}
                
                // Move preview (if cell selected)
                <Show when=move || selected_cell.get().is_some()>
                    {move || {
                        let (sx, sy) = selected_cell.get().unwrap();
                        view! {
                            <g pointer-events="none">
                                // Highlight selected cell
                                <rect
                                    x=sx as f32 * cell_size
                                    y=sy as f32 * cell_size
                                    width=cell_size
                                    height=cell_size
                                    fill="none"
                                    stroke="#4CAF50"
                                    stroke-width="3"
                                    stroke-dasharray="5,5"
                                >
                                    <animate
                                        attributeName="stroke-dashoffset"
                                        values="0;10"
                                        dur="1s"
                                        repeatCount="indefinite"
                                    />
                                </rect>
                            </g>
                        }
                    }}
                </Show>
            </svg>
            
            // Board legend
            <div class="board-legend">
                <div class="legend-item">
                    <span class="legend-symbol attractor">{"⊕"}</span>
                    <span>"Attractor"</span>
                </div>
                <div class="legend-item">
                    <span class="legend-symbol repulsor">{"⊖"}</span>
                    <span>"Repulsor"</span>
                </div>
                <div class="legend-item">
                    <span class="legend-symbol automaton">{"◉"}</span>
                    <span>"Automaton"</span>
                </div>
                <Show when=move || highlight_goals.get()>
                    <div class="legend-item">
                        <span class="legend-symbol goal-p1">{"★"}</span>
                        <span>"P1 Goal"</span>
                    </div>
                    <div class="legend-item">
                        <span class="legend-symbol goal-p2">{"★"}</span>
                        <span>"P2 Goal"</span>
                    </div>
                </Show>
            </div>
        </div>
    }
}

fn render_cell_content(cell: &CellState, x: u8, y: u8, cell_size: f32, is_automaton: bool) -> impl IntoView {
    let cx = x as f32 * cell_size + cell_size * 0.5;
    let cy = y as f32 * cell_size + cell_size * 0.5;
    let radius = cell_size * 0.35;
    
    if is_automaton {
        // Automaton gets special rendering
        view! {
            <g>
                <circle
                    cx=cx
                    cy=cy
                    r=radius
                    fill="#FFD700"
                    stroke="#FF8C00"
                    stroke-width="2"
                    class="automaton-piece"
                >
                    // Pulsing animation
                    <animate
                        attributeName="r"
                        values=format!("{};{};{}", radius * 0.9, radius * 1.1, radius * 0.9)
                        dur="2s"
                        repeatCount="indefinite"
                    />
                </circle>
                <circle
                    cx=cx
                    cy=cy
                    r=radius * 0.5
                    fill="#FFA500"
                />
            </g>
        }
    } else if let Some(particle_type) = &cell.particle {
        match particle_type.as_str() {
            "attractor" => view! {
                <g>
                    <circle
                        cx=cx
                        cy=cy
                        r=radius
                        fill="#4CAF50"
                        stroke="#388E3C"
                        stroke-width="2"
                        class="attractor-piece"
                    />
                    <text
                        x=cx
                        y=cy
                        text-anchor="middle"
                        dominant-baseline="central"
                        font-size="24"
                        font-weight="bold"
                        fill="white"
                        pointer-events="none"
                    >
                        "+"
                    </text>
                </g>
            },
            "repulsor" => view! {
                <g>
                    <circle
                        cx=cx
                        cy=cy
                        r=radius
                        fill="#F44336"
                        stroke="#D32F2F"
                        stroke-width="2"
                        class="repulsor-piece"
                    />
                    <text
                        x=cx
                        y=cy
                        text-anchor="middle"
                        dominant-baseline="central"
                        font-size="24"
                        font-weight="bold"
                        fill="white"
                        pointer-events="none"
                    >
                        "−"
                    </text>
                </g>
            },
            _ => view! { <g></g> },
        }
    } else {
        view! { <g></g> }
    }
}

fn star_path(cx: f32, cy: f32, r: f32) -> String {
    let mut path = String::new();
    for i in 0..5 {
        let angle = (i as f32 * 72.0 - 90.0).to_radians();
        let x = cx + r * angle.cos();
        let y = cy + r * angle.sin();
        
        if i == 0 {
            path.push_str(&format!("M {} {} ", x, y));
        } else {
            path.push_str(&format!("L {} {} ", x, y));
        }
        
        let inner_angle = ((i as f32 * 72.0 + 36.0) - 90.0).to_radians();
        let inner_x = cx + (r * 0.5) * inner_angle.cos();
        let inner_y = cy + (r * 0.5) * inner_angle.sin();
        path.push_str(&format!("L {} {} ", inner_x, inner_y));
    }
    path.push('Z');
    path
}
