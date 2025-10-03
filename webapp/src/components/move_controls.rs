// MoveControls component - handles move input and submission
use crate::{components::use_toast, helpers::create_api_client, state::AppState};
use automatafl_api_types::GameStateResponse;
use automatafl_logic::{Coord, MoveFeedback};
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use uuid::Uuid;

#[component]
pub fn MoveControls(game_id: Uuid, game_state: GameStateResponse) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();

    let (from_x, set_from_x) = signal(String::new());
    let (from_y, set_from_y) = signal(String::new());
    let (to_x, set_to_x) = signal(String::new());
    let (to_y, set_to_y) = signal(String::new());

    // Store timeout handle for cancellation (Rc<RefCell<>> for interior mutability)
    let optimistic_timeout: Rc<RefCell<Option<gloo_timers::callback::Timeout>>> =
        Rc::new(RefCell::new(None));

    // Check if it's this player's turn
    let current_player_id = app_state.current_player_id.get();
    let my_pid = current_player_id.and_then(|id| game_state.player_ids.get(&id).copied());

    // Check if player has already submitted a move this round
    let has_pending_move = my_pid
        .map(|pid| game_state.game.pending_moves.iter().any(|mv| mv.who == pid))
        .unwrap_or(false);

    let submit_move_action =
        Action::new_local(move |(gid, fx, fy, tx, ty): &(Uuid, u8, u8, u8, u8)| {
            let gid = *gid;
            let from = Coord { x: *fx, y: *fy };
            let to = Coord { x: *tx, y: *ty };
            async move {
                let client = create_api_client();
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

    let toast_clone = toast.clone();
    let timeout_ref_submit = optimistic_timeout.clone();
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        if my_pid.is_none() {
            toast_clone.info("You are not a player in this game");
            return;
        }

        if has_pending_move {
            toast_clone.info("You have already submitted a move this round");
            return;
        }

        // Parse coordinates
        let Ok(fx) = from_x.get().parse::<u8>() else {
            toast_clone.error("Invalid from X coordinate (must be a number)");
            return;
        };
        let Ok(fy) = from_y.get().parse::<u8>() else {
            toast_clone.error("Invalid from Y coordinate (must be a number)");
            return;
        };
        let Ok(tx) = to_x.get().parse::<u8>() else {
            toast_clone.error("Invalid to X coordinate (must be a number)");
            return;
        };
        let Ok(ty) = to_y.get().parse::<u8>() else {
            toast_clone.error("Invalid to Y coordinate (must be a number)");
            return;
        };

        // Cancel any existing optimistic timeout before submitting
        if let Some(timeout) = timeout_ref_submit.borrow_mut().take() {
            timeout.cancel();
        }

        toast_clone.info("Submitting move...");
        submit_move_action.dispatch((game_id, fx, fy, tx, ty));
    };

    let toast_clone2 = toast.clone();
    let timeout_ref_effect = optimistic_timeout.clone();
    Effect::new(move |_| {
        if let Some(result) = submit_move_action.value().get() {
            // Cancel optimistic timeout on completion (success or failure)
            if let Some(timeout) = timeout_ref_effect.borrow_mut().take() {
                timeout.cancel();
            }

            match result {
                Ok(move_result) => {
                    match move_result.feedback {
                        MoveFeedback::Committed => {
                            toast_clone2.success("Move submitted! Waiting for other players...");
                            // Clear form
                            set_from_x.set(String::new());
                            set_from_y.set(String::new());
                            set_to_x.set(String::new());
                            set_to_y.set(String::new());
                        }
                        MoveFeedback::MustMove => {
                            toast_clone2.error("Source and destination must be different");
                        }
                        MoveFeedback::AxisAlignedOnly => {
                            toast_clone2
                                .error("Move must be along a row or column (like a Rook in chess)");
                        }
                        MoveFeedback::WaitYourTurn => {
                            toast_clone2.warning("Wait for conflict resolution to complete");
                        }
                        MoveFeedback::GameOver => {
                            toast_clone2.info("Game is already over");
                        }
                        MoveFeedback::SeeCoords(details) => {
                            toast_clone2.error(format!("Invalid move: {}", details));
                        }
                    }
                }
                Err(e) => {
                    toast_clone2.error(format!("Network error: {}", e));
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
