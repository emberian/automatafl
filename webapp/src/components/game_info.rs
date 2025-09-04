use automatafl_api::GameStateResponse;
use leptos::prelude::*;
use crate::state::AppState;

#[component]
pub fn GameInfo(game_state: Signal<Option<GameStateResponse>>) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let current_user = app_state.current_user;
    
    view! {
        <div class="game-info-panel">
            <h3>"Game Information"</h3>
            
            {move || {
                if let Some(game) = game_state.get() {
                    let current_user_id = current_user.get().map(|u| u.id);
                    let is_player = current_user_id.map(|id| {
                        game.white_player.as_ref().map(|p| p.id == id).unwrap_or(false) ||
                        game.black_player.as_ref().map(|p| p.id == id).unwrap_or(false)
                    }).unwrap_or(false);
                    
                    let is_white = current_user_id.and_then(|id| {
                        game.white_player.as_ref().map(|p| p.id == id)
                    }).unwrap_or(false);
                    
                    let is_black = current_user_id.and_then(|id| {
                        game.black_player.as_ref().map(|p| p.id == id)
                    }).unwrap_or(false);
                    
                    view! {
                        <div class="game-info-content">
                            // Players section
                            <div class="info-section">
                                <h4>"Players"</h4>
                                <div class="player-info white-player">
                                    <span class="player-color">{"⚪"}</span>
                                    <span class="player-name">
                                        {game.white_player.as_ref()
                                            .map(|p| format!("{} ({})", p.username, p.rating))
                                            .unwrap_or_else(|| "Waiting for player...".to_string())}
                                    </span>
                                    <Show when=move || is_white>
                                        <span class="you-badge">"(You)"</span>
                                    </Show>
                                </div>
                                <div class="player-info black-player">
                                    <span class="player-color">{"⚫"}</span>
                                    <span class="player-name">
                                        {game.black_player.as_ref()
                                            .map(|p| format!("{} ({})", p.username, p.rating))
                                            .unwrap_or_else(|| "Waiting for player...".to_string())}
                                    </span>
                                    <Show when=move || is_black>
                                        <span class="you-badge">"(You)"</span>
                                    </Show>
                                </div>
                            </div>
                            
                            // Game status section
                            <div class="info-section">
                                <h4>"Status"</h4>
                                <div class="status-info">
                                    <div class="status-item">
                                        <span class="status-label">"Round:"</span>
                                        <span class="status-value">{&game.round_state}</span>
                                    </div>
                                    <Show when=move || game.current_player_turn.is_some()>
                                        <div class="status-item">
                                            <span class="status-label">"Turn:"</span>
                                            <span class="status-value">
                                                {if game.current_player_turn == Some(1) { "White" } else { "Black" }}
                                            </span>
                                        </div>
                                    </Show>
                                    <Show when=move || game.spectator_count > 0>
                                        <div class="status-item">
                                            <span class="status-label">"Spectators:"</span>
                                            <span class="status-value">{game.spectator_count}</span>
                                        </div>
                                    </Show>
                                </div>
                            </div>
                            
                            // Winner section
                            <Show when=move || game.winner.is_some()>
                                <div class="info-section winner-section">
                                    <h4>"Game Over"</h4>
                                    <div class="winner-info">
                                        {move || {
                                            let winner_id = game.winner.unwrap();
                                            let winner_name = if let Some(white) = &game.white_player {
                                                if white.id == winner_id {
                                                    format!("White ({})", white.username)
                                                } else if let Some(black) = &game.black_player {
                                                    format!("Black ({})", black.username)
                                                } else {
                                                    "Unknown".to_string()
                                                }
                                            } else {
                                                "Unknown".to_string()
                                            };
                                            
                                            view! {
                                                <p class="winner-text">
                                                    "🏆 Winner: " {winner_name}
                                                </p>
                                            }
                                        }}
                                    </div>
                                </div>
                            </Show>
                            
                            // Your role section
                            <Show when=move || is_player>
                                <div class="info-section">
                                    <h4>"Your Role"</h4>
                                    <div class="role-info">
                                        <p>
                                            "You are playing as "
                                            <strong>{if is_white { "White" } else { "Black" }}</strong>
                                        </p>
                                        <p class="goal-info">
                                            "Your goal is marked with a "
                                            <span class="goal-marker">{if is_white { "⚪" } else { "⚫" }}</span>
                                            " on the board"
                                        </p>
                                    </div>
                                </div>
                            </Show>
                            
                            // Instructions
                            <div class="info-section">
                                <h4>"How to Play"</h4>
                                <ol class="instructions-list">
                                    <li>"Click a particle to select it"</li>
                                    <li>"Click an empty cell to move"</li>
                                    <li>"Guide the automaton to your goal"</li>
                                    <li>"First to reach their goal wins!"</li>
                                </ol>
                            </div>
                        </div>
                    }
                } else {
                    view! {
                        <div class="game-info-loading">
                            <p>"Loading game information..."</p>
                        </div>
                    }
                }
            }}
        </div>
    }
}
