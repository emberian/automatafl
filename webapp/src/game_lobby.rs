use crate::api::ApiClient;
use automatafl_api::{GameStateResponse, GameStatus};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn GameLobby(
    api: ApiClient,
    on_select_game: impl Fn(Uuid) + 'static,
) -> impl IntoView {
    let (games, set_games) = create_signal(Vec::<GameStateResponse>::new());
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(None::<String>);
    let (creating_game, set_creating_game) = create_signal(false);

    // Load games on mount
    create_effect(move |_| {
        let api = api.clone();
        spawn_local(async move {
            set_loading.set(true);
            match api.list_games().await {
                Ok(game_list) => {
                    set_games.set(game_list);
                    set_error.set(None);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            set_loading.set(false);
        });
    });

    // Auto-refresh games every 5 seconds
    let api_clone = api.clone();
    create_effect(move |_| {
        let handle = gloo_timers::callback::Interval::new(5000, move || {
            let api = api_clone.clone();
            spawn_local(async move {
                if let Ok(game_list) = api.list_games().await {
                    set_games.set(game_list);
                }
            });
        });
        
        on_cleanup(move || {
            drop(handle);
        });
    });

    let create_game = move |_| {
        let api = api.clone();
        let on_select = on_select_game.clone();
        
        spawn_local(async move {
            set_creating_game.set(true);
            match api.create_game(None).await {
                Ok(game) => {
                    on_select(game.id);
                }
                Err(e) => {
                    set_error.set(Some(format!("Failed to create game: {}", e)));
                }
            }
            set_creating_game.set(false);
        });
    };

    view! {
        <div class="game-lobby">
            <div class="lobby-header">
                <h2>"Game Lobby"</h2>
                <button
                    class="btn btn-primary"
                    on:click=create_game
                    disabled=creating_game
                >
                    {move || if creating_game.get() { "Creating..." } else { "Create New Game" }}
                </button>
            </div>

            {move || error.get().map(|e| view! {
                <div class="alert alert-error">{e}</div>
            })}

            {move || if loading.get() {
                view! {
                    <div class="loading">
                        <p>"Loading games..."</p>
                    </div>
                }.into_view()
            } else if games.get().is_empty() {
                view! {
                    <div class="empty-state">
                        <p>"No games available. Create one to get started!"</p>
                    </div>
                }.into_view()
            } else {
                view! {
                    <div class="games-grid">
                        <For
                            each=move || games.get()
                            key=|game| game.id
                            children=move |game| {
                                view! {
                                    <GameCard
                                        game=game
                                        on_click=on_select_game.clone()
                                    />
                                }
                            }
                        />
                    </div>
                }.into_view()
            }}
        </div>
    }
}

#[component]
fn GameCard(
    game: GameStateResponse,
    on_click: impl Fn(Uuid) + 'static,
) -> impl IntoView {
    let status_class = match game.round_state.as_str() {
        "waiting" => "status-waiting",
        "active" => "status-active",
        "completed" => "status-completed",
        _ => "",
    };

    let player_count = [&game.white_player, &game.black_player]
        .iter()
        .filter(|p| p.is_some())
        .count();

    let can_join = player_count < 2 && game.winner.is_none();

    view! {
        <div class="game-card" on:click=move |_| on_click(game.id)>
            <div class="game-card-header">
                <span class="game-id">{game.id.to_string()[..8].to_string()}</span>
                <span class={format!("game-status {}", status_class)}>
                    {&game.round_state}
                </span>
            </div>
            
            <div class="game-card-body">
                <div class="player-list">
                    <div class="player-slot">
                        {game.white_player.as_ref().map(|p| view! {
                            <span class="player-name">"P1: " {&p.username}</span>
                        }).unwrap_or_else(|| view! {
                            <span class="player-empty">"P1: [Empty]"</span>
                        })}
                    </div>
                    <div class="player-slot">
                        {game.black_player.as_ref().map(|p| view! {
                            <span class="player-name">"P2: " {&p.username}</span>
                        }).unwrap_or_else(|| view! {
                            <span class="player-empty">"P2: [Empty]"</span>
                        })}
                    </div>
                </div>
                
                {game.winner.map(|winner_id| {
                    let winner_name = game.white_player.as_ref()
                        .filter(|p| p.id == winner_id)
                        .or(game.black_player.as_ref().filter(|p| p.id == winner_id))
                        .map(|p| p.username.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    
                    view! {
                        <div class="game-winner">
                            "Winner: " <strong>{winner_name}</strong>
                        </div>
                    }
                })}
                
                <div class="game-card-footer">
                    <span class="spectator-count">
                        {game.spectator_count} " spectators"
                    </span>
                    {can_join.then(|| view! {
                        <span class="join-indicator">"Click to join!"</span>
                    })}
                </div>
            </div>
        </div>
    }
}
