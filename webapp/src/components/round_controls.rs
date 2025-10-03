// RoundControls component - manual round completion and game flow controls
use crate::{api::ApiClient, state::AppState};
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
    
    let api_base_url = app_state.api_base_url.clone();
    let complete_round_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.complete_round(gid).await
        }
    });
    
    Effect::new(move |_| {
        if let Some(result) = complete_round_action.value().get() {
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
    
    // Check game state for round completion readiness
    let pending_moves_count = game_state.game.pending_moves.len();
    let total_players = game_state.player_ids.len();
    let all_moves_submitted = pending_moves_count == total_players;
    
    // Check if current player has submitted a move
    let my_move = my_pid.and_then(|pid| {
        game_state.game.pending_moves.iter()
            .find(|mv| mv.who == pid)
            .cloned()
    });
    
    view! {
        <div class="round-controls">
            <h3>"Round Control"</h3>
            
            <div class="round-status">
                <div class="status-item">
                    <span class="status-label">"Moves Submitted:"</span>
                    <span class="status-value">{pending_moves_count}" / "{total_players}</span>
                </div>
                
                <div class="status-item">
                    <span class="status-label">"Round Status:"</span>
                    <span class=format!("status-value {}", if all_moves_submitted { "ready" } else { "waiting" })>
                        {if all_moves_submitted { "Ready to Complete" } else { "Waiting for Moves" }}
                    </span>
                </div>
            </div>
            
            {if is_player {
                view! {
                    <div class="round-actions">
                        <button
                            class="button button-primary"
                            on:click=move |_| { let _ = complete_round_action.dispatch(game_id); }
                            disabled=move || complete_round_action.pending().get() || !all_moves_submitted
                        >
                            {move || if complete_round_action.pending().get() { "Completing..." } else { "Complete Round" }}
                        </button>
                        
                        {if !all_moves_submitted {
                            view! {
                                <p class="round-hint">
                                    "Waiting for all players to submit their moves before the round can be completed."
                                </p>
                            }.into_any()
                        } else {
                            view! {
                                <p class="round-hint">
                                    "All moves submitted! Click 'Complete Round' to process moves and advance the automaton."
                                </p>
                            }.into_any()
                        }}
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="spectator-info">
                        <p>"You are spectating this game. Round completion is automatic or controlled by players."</p>
                    </div>
                }.into_any()
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
            
            <div class="pending-moves">
                <h4>"Your Move"</h4>
                {if let Some(mv) = my_move {
                    view! {
                        <div class="pending-move">
                            <span class="move-player">"Player " {mv.who.0}</span>
                            <span class="move-coords">"(" {mv.from.x} "," {mv.from.y} ") → (" {mv.to.x} "," {mv.to.y} ")"</span>
                        </div>
                    }.into_any()
                } else if is_player {
                    view! {
                        <p class="no-moves">"You haven't submitted a move yet"</p>
                    }.into_any()
                } else {
                    view! {
                        <p class="no-moves">"Spectators cannot see pending moves"</p>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

