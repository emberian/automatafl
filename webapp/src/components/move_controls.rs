// MoveControls component - handles move input and submission
use crate::{api::ApiClient, state::AppState};
use automatafl_api_types::GameStateResponse;
use automatafl_logic::{Coord, MoveFeedback};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn MoveControls(game_id: Uuid, game_state: GameStateResponse) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (from_x, set_from_x) = signal(String::new());
    let (from_y, set_from_y) = signal(String::new());
    let (to_x, set_to_x) = signal(String::new());
    let (to_y, set_to_y) = signal(String::new());
    let (status, set_status) = signal(String::new());
    let (status_type, set_status_type) = signal("info".to_string()); // "success", "error", "info", "warning"
    
    // Check if it's this player's turn
    let current_player_id = app_state.current_player_id.get();
    let my_pid = current_player_id.and_then(|id| game_state.player_ids.get(&id).copied());
    
    // Check if player has already submitted a move this round
    let has_pending_move = my_pid.map(|pid| {
        game_state.game.pending_moves.iter().any(|mv| mv.who == pid)
    }).unwrap_or(false);
    
    let api_base_url = app_state.api_base_url.clone();
    let submit_move_action = Action::new_local(move |(gid, fx, fy, tx, ty): &(Uuid, u8, u8, u8, u8)| {
        let gid = *gid;
        let from = Coord { x: *fx, y: *fy };
        let to = Coord { x: *tx, y: *ty };
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.perform_move(gid, from, to).await
        }
    });
    
    let can_submit = move || {
        my_pid.is_some() 
            && !has_pending_move
            && !submit_move_action.pending().get()
            && !from_x.get().is_empty()
            && !from_y.get().is_empty()
            && !to_x.get().is_empty()
            && !to_y.get().is_empty()
    };
    
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        
        if my_pid.is_none() {
            set_status.set("ℹ️ You are not a player in this game".to_string());
            set_status_type.set("info".to_string());
            return;
        }
        
        if has_pending_move {
            set_status.set("✓ You have already submitted a move this round".to_string());
            set_status_type.set("info".to_string());
            return;
        }
        
        // Parse coordinates
        let Ok(fx) = from_x.get().parse::<u8>() else {
            set_status.set("❌ Invalid from X coordinate (must be a number)".to_string());
            set_status_type.set("error".to_string());
            return;
        };
        let Ok(fy) = from_y.get().parse::<u8>() else {
            set_status.set("❌ Invalid from Y coordinate (must be a number)".to_string());
            set_status_type.set("error".to_string());
            return;
        };
        let Ok(tx) = to_x.get().parse::<u8>() else {
            set_status.set("❌ Invalid to X coordinate (must be a number)".to_string());
            set_status_type.set("error".to_string());
            return;
        };
        let Ok(ty) = to_y.get().parse::<u8>() else {
            set_status.set("❌ Invalid to Y coordinate (must be a number)".to_string());
            set_status_type.set("error".to_string());
            return;
        };
        
        set_status.set("⏳ Submitting move...".to_string());
        set_status_type.set("info".to_string());
        submit_move_action.dispatch((game_id, fx, fy, tx, ty));
    };
    
    Effect::new(move |_| {
        if let Some(result) = submit_move_action.value().get() {
            match result {
                Ok(move_result) => {
                    match move_result.feedback {
                        MoveFeedback::Committed => {
                            set_status.set("✅ Move committed! Waiting for other players...".to_string());
                            set_status_type.set("success".to_string());
                            // Clear form
                            set_from_x.set(String::new());
                            set_from_y.set(String::new());
                            set_to_x.set(String::new());
                            set_to_y.set(String::new());
                        }
                        MoveFeedback::MustMove => {
                            set_status.set("❌ Source and destination must be different".to_string());
                            set_status_type.set("error".to_string());
                        }
                        MoveFeedback::AxisAlignedOnly => {
                            set_status.set("❌ Move must be along a row or column (like a Rook in chess)".to_string());
                            set_status_type.set("error".to_string());
                        }
                        MoveFeedback::WaitYourTurn => {
                            set_status.set("⏳ Wait for conflict resolution to complete".to_string());
                            set_status_type.set("warning".to_string());
                        }
                        MoveFeedback::GameOver => {
                            set_status.set("🏁 Game is already over".to_string());
                            set_status_type.set("info".to_string());
                        }
                        MoveFeedback::SeeCoords(details) => {
                            set_status.set(format!("❌ Invalid move: {}", details));
                            set_status_type.set("error".to_string());
                        }
                    }
                }
                Err(e) => {
                    set_status.set(format!("🔥 Network error: {}", e));
                    set_status_type.set("error".to_string());
                }
            }
        }
    });
    
    view! {
        <div class="move-controls">
            <h3>"Submit Move"</h3>
            
            {if has_pending_move {
                view! {
                    <div class="info-message">
                        <p>"✓ Move submitted! Waiting for other players to submit their moves."</p>
                        <p class="form-hint">"The round will automatically progress once all players have submitted their moves."</p>
                    </div>
                }.into_any()
            } else if my_pid.is_some() {
                view! {
                    <form on:submit=on_submit class="move-form">
                        <div class="coordinate-inputs">
                            <div class="coord-group">
                                <label>"From"</label>
                                <div class="coord-inputs">
                                    <input
                                        type="number"
                                        placeholder="X"
                                        min="0"
                                        max=game_state.game.board.size.x - 1
                                        class="coord-input"
                                        on:input=move |ev| set_from_x.set(event_target_value(&ev))
                                        prop:value=from_x
                                    />
                                    <input
                                        type="number"
                                        placeholder="Y"
                                        min="0"
                                        max=game_state.game.board.size.y - 1
                                        class="coord-input"
                                        on:input=move |ev| set_from_y.set(event_target_value(&ev))
                                        prop:value=from_y
                                    />
                                </div>
                            </div>
                            
                            <span class="arrow">"→"</span>
                            
                            <div class="coord-group">
                                <label>"To"</label>
                                <div class="coord-inputs">
                                    <input
                                        type="number"
                                        placeholder="X"
                                        min="0"
                                        max=game_state.game.board.size.x - 1
                                        class="coord-input"
                                        on:input=move |ev| set_to_x.set(event_target_value(&ev))
                                        prop:value=to_x
                                    />
                                    <input
                                        type="number"
                                        placeholder="Y"
                                        min="0"
                                        max=game_state.game.board.size.y - 1
                                        class="coord-input"
                                        on:input=move |ev| set_to_y.set(event_target_value(&ev))
                                        prop:value=to_y
                                    />
                                </div>
                            </div>
                        </div>
                        
                        <button
                            type="submit"
                            class="button button-primary"
                            disabled=move || !can_submit()
                        >
                            {move || if submit_move_action.pending().get() { "Submitting..." } else { "Submit Move" }}
                        </button>
                    </form>
                    
                    {move || {
                        let s = status.get();
                        let st = status_type.get();
                        if !s.is_empty() {
                            view! {
                                <div class=format!("move-status status-{}", st)>{s}</div>
                            }.into_any()
                        } else {
                            view! {}.into_any()
                        }
                    }}
                }.into_any()
            } else {
                view! {
                    <div class="info-message">
                        "You are spectating this game"
                    </div>
                }.into_any()
            }}
        </div>
    }
}
