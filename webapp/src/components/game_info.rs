// GameInfo component - displays game status and player information
use automatafl_api_types::{GameStateResponse, GameLifecycle};
use leptos::prelude::*;

#[component]
pub fn GameInfo(game_state: GameStateResponse) -> impl IntoView {
    let (status_text, status_class) = match game_state.lifecycle {
        GameLifecycle::Waiting => ("Waiting for Players", "status-waiting"),
        GameLifecycle::InProgress => ("Game in Progress", "status-active"),
        GameLifecycle::Finished => ("Game Finished", "status-finished"),
    };
    
    let player_count = game_state.player_ids.len();
    let max_players = game_state.game.player_count;
    
    view! {
        <div class="game-info">
            <div class="game-status">
                <span class=format!("status-badge {}", status_class)>
                    {status_text}
                </span>
            </div>
            
            <div class="player-info">
                <h3>"Players"</h3>
                <div class="player-list">
                    {(0..max_players).map(|i| {
                        let has_player = (i as usize) < player_count;
                        view! {
                            <div class="player-slot">
                                <span class="player-label">"Player " {i}</span>
                                <span class="player-status">
                                    {if has_player { "✓ Joined" } else { "Waiting..." }}
                                </span>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>
            
            <div class="game-rules">
                <h3>"Rules"</h3>
                <ul>
                    <li>"Column Rule: " {if game_state.game.use_column_rule { "Enabled" } else { "Disabled" }}</li>
                    <li>"Automaton at: (" {game_state.game.board.automaton_location.x} ", " {game_state.game.board.automaton_location.y} ")"</li>
                </ul>
            </div>
            
            <Show when=move || game_state.game.winner.is_some()>
                <div class="winner-announcement">
                    <h2>"🎉 Winner: Player " {game_state.game.winner.unwrap().0}</h2>
                </div>
            </Show>
        </div>
    }
}
