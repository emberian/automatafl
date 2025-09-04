use automatafl_api::GameStateResponse;
use leptos::prelude::*;
use crate::state::AppState;

#[component]
pub fn MoveControls(
    game_state: Signal<Option<GameStateResponse>>,
    selected_cell: RwSignal<Option<(u8, u8)>>,
    on_submit_move: impl Fn(u8, u8, u8, u8) + 'static + Copy,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let current_user = app_state.current_user;
    
    let (destination_cell, set_destination_cell) = create_signal(Option::<(u8, u8)>::None);
    let (error, set_error) = create_signal(Option::<String>::None);
    
    let can_make_move = move || {
        if let Some(game) = game_state.get() {
            if game.winner.is_some() {
                return false;
            }
            
            if let Some(user) = current_user.get() {
                let is_white = game.white_player.as_ref().map(|p| p.id == user.id).unwrap_or(false);
                let is_black = game.black_player.as_ref().map(|p| p.id == user.id).unwrap_or(false);
                
                if !is_white && !is_black {
                    return false; // Not a player
                }
                
                match game.current_player_turn {
                    Some(1) => is_white,
                    Some(2) => is_black,
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        }
    };
    
    let submit_move = move |_| {
        if let (Some((from_x, from_y)), Some((to_x, to_y))) = (selected_cell.get(), destination_cell.get()) {
            on_submit_move(from_x, from_y, to_x, to_y);
            selected_cell.set(None);
            set_destination_cell(None);
            set_error(None);
        } else {
            set_error(Some("Please select both source and destination cells".to_string()));
        }
    };
    
    let clear_selection = move |_| {
        selected_cell.set(None);
        set_destination_cell(None);
        set_error(None);
    };

    view! {
        <div class="move-controls-panel">
            <h3>"Move Controls"</h3>
            
            <Show
                when=can_make_move
                fallback=|| view! {
                    <div class="move-disabled">
                        <p>"You cannot make a move at this time"</p>
                        <Show when=move || {
                            game_state.get().and_then(|g| g.winner).is_some()
                        }>
                            <p class="game-over-notice">"Game is over"</p>
                        </Show>
                    </div>
                }
            >
                <div class="move-controls">
                    // Move status
                    <div class="move-status">
                        <Show
                            when=move || selected_cell.get().is_some()
                            fallback=|| view! {
                                <p class="instruction">"Click a particle to select it"</p>
                            }
                        >
                            {move || {
                                let (x, y) = selected_cell.get().unwrap();
                                view! {
                                    <div class="selection-info">
                                        <p>"Selected: (" {x} ", " {y} ")"</p>
                                        <Show
                                            when=move || destination_cell.get().is_none()
                                            fallback=move || {
                                                let (dx, dy) = destination_cell.get().unwrap();
                                                view! {
                                                    <p>"Destination: (" {dx} ", " {dy} ")"</p>
                                                }
                                            }
                                        >
                                            <p class="instruction">"Click an empty cell to move there"</p>
                                        </Show>
                                    </div>
                                }
                            }}
                        </Show>
                    </div>
                    
                    // Error display
                    <Show when=move || error.get().is_some()>
                        <div class="move-error">
                            {move || error.get().unwrap_or_default()}
                        </div>
                    </Show>
                    
                    // Action buttons
                    <div class="move-actions">
                        <button
                            class="button button-primary"
                            on:click=submit_move
                            disabled=move || selected_cell.get().is_none() || destination_cell.get().is_none()
                        >
                            "Submit Move"
                        </button>
                        <button
                            class="button button-secondary"
                            on:click=clear_selection
                            disabled=move || selected_cell.get().is_none()
                        >
                            "Clear Selection"
                        </button>
                    </div>
                    
                    // Quick move guide
                    <div class="move-guide">
                        <h4>"Quick Guide"</h4>
                        <ul>
                            <li>"⊕ Attractors pull the automaton"</li>
                            <li>"⊖ Repulsors push the automaton"</li>
                            <li>"◉ The automaton moves after both players"</li>
                            <li>"Move particles to empty cells only"</li>
                        </ul>
                    </div>
                </div>
            </Show>
        </div>
    }
}
