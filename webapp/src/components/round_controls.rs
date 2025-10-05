// RoundControls component - manual round completion and game flow controls
use crate::state::AppState;
use automatafl_api_types::GameStateResponse;
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn RoundControls(game_id: Uuid, game_state: GameStateResponse) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");

    let (status, set_status) = signal(String::new());

    // Check if current player can control rounds (admin or player in game)
    let current_player_id = app_state.current_player_id.get();
    let my_pid = current_player_id.and_then(|id| game_state.player_ids.get(&id).copied());
    let is_player = my_pid.is_some();

    let app_state_for_complete = app_state.clone();
    let complete_round_action = Action::new_local(move |gid: &Uuid| {
        let app_state = app_state_for_complete.clone();
        let gid = *gid;
        async move {
            let client = app_state.get_api_client();
            client.complete_round(gid).await
        }
    });

    Effect::new(move |_| {
        complete_round_action.value().with(|result| {
            if let Some(result) = result {
                match result {
                    Ok(_) => {
                        set_status.set("✅ Round completed successfully!".to_string());
                    }
                    Err(e) => {
                        set_status.set(format!("❌ Failed to complete round: {}", e));
                    }
                }
            }
        });
    });

    // Check game state for round completion readiness
    let pending_moves_count = game_state.game.pending_moves.len();
    let total_players = game_state.player_ids.len();
    let all_moves_submitted = pending_moves_count == total_players;

    // Check if current player has submitted a move
    let my_move = my_pid.and_then(|pid| {
        game_state
            .game
            .pending_moves
            .iter()
            .find(|mv| mv.who == pid)
            .cloned()
    });

    view! {
        <div class="round-controls">
            <h3>"Round " {format!("{:?}", game_state.game.round)}</h3>

            <div class="round-progress">
                <div class="progress-bar-container">
                    <div
                        class="progress-bar-fill"
                        style=format!("width: {}%", (pending_moves_count as f32 / total_players as f32 * 100.0))
                    />
                </div>
                <div class="progress-text">
                    {pending_moves_count}" / "{total_players}" moves submitted"
                </div>
            </div>

            {if is_player {
                if let Some(mv) = my_move {
                    view! {
                        <div class="my-pending-move">
                            <span class="move-status-icon">"✓"</span>
                            <span class="move-details">
                                "Your move: (" {mv.from.x}","  {mv.from.y}") → (" {mv.to.x}"," {mv.to.y}")"
                            </span>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="my-pending-move awaiting">
                            <span class="move-status-icon">"⏳"</span>
                            <span class="move-details">"Submit your move to continue"</span>
                        </div>
                    }.into_any()
                }
            } else {
                view! {}.into_any()
            }}

            {if is_player && all_moves_submitted {
                view! {
                    <div class="round-complete-section">
                        <button
                            class="button button-primary button-block"
                            on:click=move |_| { let _ = complete_round_action.dispatch(game_id); }
                            disabled=move || complete_round_action.pending().get()
                        >
                            {move || if complete_round_action.pending().get() {
                                "⏳ Processing..."
                            } else {
                                "▶️ Complete Round"
                            }}
                        </button>
                        <p class="round-hint">"All moves ready! Execute the round."</p>
                    </div>
                }.into_any()
            } else if !is_player {
                view! {
                    <div class="spectator-note">
                        <p>"👁️ Spectating - rounds complete automatically"</p>
                    </div>
                }.into_any()
            } else {
                view! {}.into_any()
            }}

            {move || {
                let s = status.get();
                if !s.is_empty() {
                    view! {
                        <div class="round-status-message">{s}</div>
                    }.into_any()
                } else {
                    view! {}.into_any()
                }
            }}
        </div>
    }
}
