// GameHistory component - displays game events and history
// NOW READS FROM REACTIVE SIGNALS - NO HTTP REQUESTS!
use crate::state::AppState;
use automatafl_api_types::{GameEvent, GameEventData};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn GameHistory(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");

    let (filter_event_kind, set_filter_event_kind) = signal(Option::<String>::None);

    // Get reactive event log signal - updates automatically from WebSocket!
    // NO HTTP REQUESTS - data comes from signals updated by handle_game_event
    let get_filtered_events = move || {
        app_state
            .get_event_log_signal(game_id)
            .map(|sig| {
                let events = sig.get();
                // Apply filter reactively
                if let Some(kind) = filter_event_kind.get() {
                    events
                        .into_iter()
                        .filter(|e| event_matches_kind(e, &kind))
                        .collect::<Vec<_>>()
                } else {
                    events
                }
            })
            .unwrap_or_default()
    };

    view! {
        <div class="game-history">
            <div class="history-header">
                <h3>"Game History"</h3>
                <div class="history-controls">
                    <select
                        class="history-filter"
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            if value.is_empty() || value == "all" {
                                set_filter_event_kind.set(None);
                            } else {
                                set_filter_event_kind.set(Some(value));
                            }
                        }
                    >
                        <option value="all">"All Events"</option>
                        <option value="MOVE">"Moves"</option>
                        <option value="AUTOMATON_STEP">"Automaton Steps"</option>
                        <option value="ROUND_COMPLETE">"Round Completions"</option>
                        <option value="CONFLICTS">"Conflicts"</option>
                        <option value="GAME_OVER">"Game Over"</option>
                    </select>
                </div>
            </div>

            <div class="history-events">
                {move || {
                    let events = get_filtered_events();
                    if events.is_empty() {
                        view! {
                            <div class="history-empty">
                                <p>"No events yet"</p>
                            </div>
                        }.into_any()
                    } else {
                        events.into_iter().map(|event| {
                            view! { <GameEventItem event=event /> }
                        }).collect_view().into_any()
                    }
                }}
            </div>
        </div>
    }
}

/// Helper to match events by type for filtering
fn event_matches_kind(event: &GameEvent, kind: &str) -> bool {
    match (kind, &event.data) {
        ("MOVE", GameEventData::Move { .. }) => true,
        ("AUTOMATON_STEP", GameEventData::AutomatonStep { .. }) => true,
        ("ROUND_COMPLETE", GameEventData::RoundComplete { .. }) => true,
        ("CONFLICTS", GameEventData::Conflicts { .. }) => true,
        ("GAME_OVER", GameEventData::GameOver { .. }) => true,
        _ => false,
    }
}

#[component]
fn GameEventItem(event: GameEvent) -> impl IntoView {
    let (event_type, event_content) = match &event.data {
        GameEventData::PlayerJoined {
            player_pid,
            displayname,
            ..
        } => (
            "player-joined",
            format!("Player {} ({}) joined the game", displayname, player_pid.0),
        ),
        GameEventData::GameStarted { .. } => ("game-started", "Game has started!".to_string()),
        GameEventData::MoveAcknowledged {
            player_pid,
            from,
            to,
            ..
        } => (
            "move-ack",
            format!(
                "Player {} submitted move: ({},{}) → ({},{})",
                player_pid.0, from.x, from.y, to.x, to.y
            ),
        ),
        GameEventData::MoveInvalid {
            player_pid,
            feedback,
        } => (
            "move-invalid",
            format!("Player {} invalid move: {:?}", player_pid.0, feedback),
        ),
        GameEventData::Move {
            player_pid,
            from,
            to,
            result,
        } => (
            "move",
            format!(
                "Player {} moved: ({},{}) → ({},{}). Result: {:?}",
                player_pid.0, from.x, from.y, to.x, to.y, result
            ),
        ),
        GameEventData::AutomatonStep { location } => (
            "automaton-step",
            format!("Automaton moved to ({}, {})", location.x, location.y),
        ),
        GameEventData::GameOver { winner, .. } => {
            ("game-over", format!("Game Over! Player {} wins!", winner.0))
        }
        GameEventData::EloUpdate { changes } => {
            let details = changes
                .iter()
                .map(|c| format!("{:+}", c.change))
                .collect::<Vec<_>>()
                .join(", ");
            ("elo-update", format!("ELO updated: {}", details))
        }
        GameEventData::RoundComplete { .. } => {
            ("round-complete", "Round completed successfully".to_string())
        }
        GameEventData::Conflicts {
            locked_players,
            conflict_coords,
        } => (
            "conflicts",
            format!(
                "Conflicts! {} players locked, {} positions in conflict",
                locked_players.len(),
                conflict_coords.len()
            ),
        ),
        GameEventData::Chat {
            displayname,
            message,
            ..
        } => ("chat", format!("{}: {}", displayname, message)),
        GameEventData::GameLoaded { snapshot_index } => (
            "game-loaded",
            format!("Game loaded from snapshot #{}", snapshot_index),
        ),
        GameEventData::State { .. } => ("state", "Full game state received".to_string()),
    };

    let timestamp_display = event
        .timestamp
        .map(|ts| {
            chrono::DateTime::from_timestamp(ts as i64, 0)
                .map(|dt| dt.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown".to_string())
        })
        .unwrap_or_else(|| "No timestamp".to_string());

    view! {
        <div class=format!("history-event event-{}", event_type)>
            <span class="event-time">{timestamp_display}</span>
            <span class="event-content">{event_content}</span>
        </div>
    }
}
