use crate::state::AppState;
use automatafl_api_types::LeaderboardEntry;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn LeaderboardPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let (active_tab, set_active_tab) = signal("elo".to_string());
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    let app_state_for_leaderboard = app_state.clone();
    let leaderboard_resource = LocalResource::new(move || {
        let _ = refresh_trigger.get();
        let tab = active_tab.get();
        let app_state = app_state_for_leaderboard.clone();
        async move {
            let client = app_state.get_api_client();
            match tab.as_str() {
                "elo" => client.get_leaderboard_elo().await,
                "wins" => client.get_leaderboard_wins().await,
                "games" => client.get_leaderboard_games().await,
                _ => client.get_leaderboard_elo().await,
            }
        }
    });

    let on_tab_change = move |tab: &str| {
        set_active_tab.set(tab.to_string());
        set_refresh_trigger.update(|n| *n += 1);
    };

    view! {
        <div class="leaderboard-page">
            <div class="page-header">
                <h1>"🏆 Leaderboard"</h1>
                <button
                    class="button button-small"
                    on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                >
                    "Refresh"
                </button>
            </div>

            <div class="leaderboard-tabs">
                <button
                    class=move || format!("tab {}", if active_tab.get() == "elo" { "active" } else { "" })
                    on:click=move |_| on_tab_change("elo")
                >
                    "ELO Rating"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "wins" { "active" } else { "" })
                    on:click=move |_| on_tab_change("wins")
                >
                    "Most Wins"
                </button>
                <button
                    class=move || format!("tab {}", if active_tab.get() == "games" { "active" } else { "" })
                    on:click=move |_| on_tab_change("games")
                >
                    "Most Games"
                </button>
            </div>

            <Suspense fallback=move || view! {
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading leaderboard..."</p>
                </div>
            }>
                {move || {
                    leaderboard_resource.get().map(|result| {
                        match result {
                            Ok(leaderboard) => {
                                if leaderboard.entries.is_empty() {
                                    view! {
                                        <div class="empty-state">
                                            <h3>"No players yet"</h3>
                                            <p>"Be the first to play some games!"</p>
                                            <A href="/games/create" attr:class="button button-primary">
                                                "Create Game"
                                            </A>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="leaderboard-table-container">
                                            <table class="leaderboard-table">
                                                <thead>
                                                    <tr>
                                                        <th class="rank-column">"Rank"</th>
                                                        <th class="player-column">"Player"</th>
                                                        <th class="stat-column">
                                                            {match active_tab.get().as_str() {
                                                                "elo" => "ELO",
                                                                "wins" => "Wins",
                                                                "games" => "Games Played",
                                                                _ => "Score"
                                                            }}
                                                        </th>
                                                        <th class="extra-column">"Win Rate"</th>
                                                        <th class="extra-column">"Total Games"</th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {leaderboard.entries.into_iter().enumerate().map(|(idx, entry)| {
                                                        view! {
                                                            <LeaderboardRow rank=idx + 1 entry=entry />
                                                        }
                                                    }).collect_view()}
                                                </tbody>
                                            </table>
                                        </div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! {
                                <div class="error-state">
                                    <h3>"Error loading leaderboard"</h3>
                                    <p>{format!("{}", e)}</p>
                                    <button
                                        class="button"
                                        on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                                    >
                                        "Retry"
                                    </button>
                                </div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn LeaderboardRow(rank: usize, entry: LeaderboardEntry) -> impl IntoView {
    let games_played = entry.games_played.unwrap_or(0);
    let games_won = entry.games_won.unwrap_or(0);
    let games_lost = games_played.saturating_sub(games_won);

    let win_rate = if games_played > 0 {
        (games_won as f64 / games_played as f64 * 100.0).round()
    } else {
        0.0
    };

    let rank_class = match rank {
        1 => "rank-gold",
        2 => "rank-silver",
        3 => "rank-bronze",
        _ => "rank-normal",
    };

    let rank_emoji = match rank {
        1 => "🥇",
        2 => "🥈",
        3 => "🥉",
        _ => "",
    };

    view! {
        <tr class="leaderboard-row">
            <td class=format!("rank-cell {}", rank_class)>
                <span class="rank-number">{rank}</span>
                {if !rank_emoji.is_empty() {
                    view! { <span class="rank-emoji">{rank_emoji}</span> }.into_any()
                } else {
                    view! {}.into_any()
                }}
            </td>
            <td class="player-cell">
                <A href=format!("/users/{}", entry.player_id) attr:class="player-name-link">
                    {entry.displayname}
                </A>
            </td>
            <td class="stat-cell">
                <strong>{entry.value}</strong>
            </td>
            <td class="extra-cell">
                {format!("{:.0}%", win_rate)}
                <span class="extra-detail">" ("{games_won}"W / "{games_lost}"L)"</span>
            </td>
            <td class="extra-cell">
                {games_played}
            </td>
        </tr>
    }
}
