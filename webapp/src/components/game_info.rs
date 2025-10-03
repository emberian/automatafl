// GameInfo component - displays game status and player information
use crate::state::AppState;
use automatafl_api_types::{GameStateResponse, GameLifecycle};
use leptos::prelude::*;

#[component]
pub fn GameInfo(game_state: GameStateResponse) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (status_text, status_class, status_icon) = match game_state.lifecycle {
        GameLifecycle::Waiting => ("Waiting for Players", "status-waiting", "⏳"),
        GameLifecycle::InProgress => ("Game in Progress", "status-active", "▶️"),
        GameLifecycle::Finished => ("Game Finished", "status-finished", "🏁"),
    };
    
    let player_count = game_state.player_ids.len();
    let max_players = game_state.game.player_count;
    let pending_moves = game_state.game.pending_moves.len();
    
    // Get player names if available
    let current_player_id = app_state.current_player_id.get();
    let my_pid = current_player_id.and_then(|id| game_state.player_ids.get(&id).copied());
    
    view! {
        <div class="game-info">
            <div class="game-status-bar">
                <div class="status-main">
                    <span class="status-icon">{status_icon}</span>
                    <span class=format!("status-badge {}", status_class)>
                        {status_text}
                    </span>
                </div>
                <div class="status-details">
                    <span class="detail-item">
                        <span class="detail-label">"Players: "</span>
                        <span class="detail-value">{player_count}" / "{max_players}</span>
                    </span>
                    {if game_state.lifecycle == GameLifecycle::InProgress {
                        view! {
                            <span class="detail-item">
                                <span class="detail-label">"Moves: "</span>
                                <span class="detail-value">{pending_moves}" / "{player_count}</span>
                            </span>
                        }.into_any()
                    } else {
                        view! {}.into_any()
                    }}
                    <span class="detail-item">
                        <span class="detail-label">"Round: "</span>
                        <span class="detail-value">{format!("{:?}", game_state.game.round)}</span>
                    </span>
                </div>
            </div>
            
            {if let Some(my_pid) = my_pid {
                view! {
                    <div class="player-identity">
                        <span class="identity-label">"You are: "</span>
                        <span class="identity-value">"Player " {my_pid.0}</span>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="player-identity spectator">
                        <span class="identity-label">"👁️ Spectating"</span>
                    </div>
                }.into_any()
            }}
            
            <Show when=move || game_state.game.winner.is_some()>
                <div class="winner-announcement">
                    <div class="winner-content">
                        <span class="winner-icon">"🏆"</span>
                        <span class="winner-text">"Player " {game_state.game.winner.unwrap().0} " Wins!"</span>
                    </div>
                </div>
            </Show>
        </div>
    }
}
