use crate::{
    components::{
        ChatPanel, GameBoard, GameHistory, GameInfo, MoveControls, RoundControls, SaveLoadControls,
        SkeletonGameBoard, SkeletonList, use_toast,
    },
    helpers::create_api_client,
    state::AppState,
    websocket::create_game_websocket,
};
use automatafl_api_types::{GameLifecycle, GameListItem};
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

#[component]
pub fn GamesListPage() -> impl IntoView {
    let (filter_status, set_filter_status) = signal("all".to_string());
    let (sort_by, set_sort_by) = signal("created_at".to_string());
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    let games_resource = LocalResource::new(move || {
        let _ = refresh_trigger.get();
        async move {
            let client = create_api_client();
            client.list_games().await
        }
    });

    view! {
        <div class="games-list-page">
            <div class="page-header">
                <h1>"Active Games"</h1>
                <div class="header-actions">
                    <button
                        class="button button-small"
                        on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                    >
                        "🔄 Refresh"
                    </button>
                    <A href="/games/create" attr:class="button button-primary">
                        "Create New Game"
                    </A>
                </div>
            </div>

            <div class="games-filter-bar">
                <div class="filter-group">
                    <label>"Status:"</label>
                    <select
                        class="filter-select"
                        on:change=move |ev| set_filter_status.set(event_target_value(&ev))
                        prop:value=filter_status
                    >
                        <option value="all">"All Games"</option>
                        <option value="waiting">"Waiting for Players"</option>
                        <option value="in_progress">"In Progress"</option>
                        <option value="finished">"Finished"</option>
                    </select>
                </div>
                <div class="filter-group">
                    <label>"Sort by:"</label>
                    <select
                        class="filter-select"
                        on:change=move |ev| set_sort_by.set(event_target_value(&ev))
                        prop:value=sort_by
                    >
                        <option value="created_at">"Newest First"</option>
                        <option value="created_at_asc">"Oldest First"</option>
                        <option value="players">"Most Players"</option>
                    </select>
                </div>
            </div>

            <Suspense fallback=move || view! {
                <SkeletonList count=6 />
            }>
                {move || {
                    // Memoize filtered and sorted games (only recomputes when inputs change)
                    let filtered_sorted_games = Memo::new(move |_| {
                        games_resource.get().and_then(|result| {
                            result.ok().map(|mut games| {
                                // Apply filtering
                                let filter = filter_status.get();
                                games.retain(|game| match filter.as_str() {
                                    "waiting" => matches!(game.lifecycle, GameLifecycle::Waiting),
                                    "in_progress" => matches!(game.lifecycle, GameLifecycle::InProgress),
                                    "finished" => matches!(game.lifecycle, GameLifecycle::Finished),
                                    _ => true,
                                });

                                // Apply sorting
                                let sort = sort_by.get();
                                match sort.as_str() {
                                    "created_at" => games.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
                                    "created_at_asc" => games.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
                                    "players" => games.sort_by(|a, b| b.player_count.cmp(&a.player_count)),
                                    _ => {}
                                }
                                games
                            })
                        })
                    });

                    match filtered_sorted_games.get() {
                        Some(games) => {
                            view! {
                                <div class="games-grid">
                                    {if games.is_empty() {
                                        view! {
                                            <div class="empty-state">
                                                <h3>"No games match your filters"</h3>
                                                <p>"Try changing the filters or create a new game."</p>
                                                <A href="/games/create" attr:class="button button-primary">
                                                    "Create Game"
                                                </A>
                                            </div>
                                        }.into_any()
                                    } else {
                                        games.into_iter().map(|game| {
                                            view! { <GameCard game=game /> }
                                        }).collect_view().into_any()
                                    }}
                                </div>
                            }.into_any()
                        }
                        None => {
                            // Error case - show error state
                            if let Some(Err(e)) = games_resource.get() {
                                view! {
                                    <div class="error-state">
                                        <h3>"Error loading games"</h3>
                                        <p>{format!("{}", e)}</p>
                                        <button
                                            class="button button-primary"
                                            on:click=move |_| set_refresh_trigger.update(|n| *n += 1)
                                        >
                                            "Retry"
                                        </button>
                                    </div>
                                }.into_any()
                            } else {
                                // Still loading
                                view! { <SkeletonList count=6 /> }.into_any()
                            }
                        }
                    }
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn GameCard(game: GameListItem) -> impl IntoView {
    let (status_class, status_text) = match game.lifecycle {
        GameLifecycle::Waiting => ("status-waiting", "Waiting for players"),
        GameLifecycle::InProgress => ("status-active", "In progress"),
        GameLifecycle::Finished => ("status-completed", "Finished"),
    };

    view! {
        <div class="game-card">
            <div class="game-card-header">
                <h3>"Game " {game.id.to_string().chars().take(8).collect::<String>()}</h3>
                <span class=format!("game-status {}", status_class)>{status_text}</span>
            </div>

            <div class="game-card-info">
                <div class="info-row">
                    <span class="info-label">"Players:"</span>
                    <span class="info-value">{game.player_count}" / "{game.max_players}</span>
                </div>
                <div class="info-row">
                    <span class="info-label">"Created:"</span>
                    <span class="info-value">
                        {chrono::DateTime::from_timestamp(game.created_at as i64, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "Unknown".to_string())}
                    </span>
                </div>
            </div>

            <div class="game-card-actions">
                <A href=format!("/games/{}", game.id) attr:class="button button-small button-primary">
                    {match game.lifecycle {
                        GameLifecycle::Waiting => "Join Game",
                        GameLifecycle::InProgress => "View Game",
                        GameLifecycle::Finished => "View Results",
                    }}
                </A>
            </div>
        </div>
    }
}

#[component]
pub fn CreateGamePage() -> impl IntoView {
    let _app_state = use_context::<AppState>().expect("AppState should be provided");
    let navigate = leptos_router::hooks::use_navigate();

    let (player_count, set_player_count) = signal(2u8);
    let (use_column_rule, set_use_column_rule) = signal(true);

    let create_action = Action::new_local(move |(pc, ucr): &(u8, bool)| {
        let pc = *pc;
        let ucr = *ucr;
        async move {
            let client = create_api_client();
            client.create_game(pc, ucr).await
        }
    });

    let join_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        async move {
            let client = create_api_client();
            client.join_game(gid).await
        }
    });

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        create_action.dispatch((player_count.get(), use_column_rule.get()));
    };

    // After game is created, join it
    Effect::new(move |_| {
        if let Some(Ok(game_id)) = create_action.value().get() {
            join_action.dispatch(game_id);
        }
    });

    // After joining, navigate to the game
    Effect::new(move |_| {
        if let Some(result) = join_action.value().get() {
            match result {
                Ok(_pid) => {
                    // Get the game_id from create_action
                    if let Some(Ok(game_id)) = create_action.value().get() {
                        navigate(&format!("/games/{}", game_id), Default::default());
                    }
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Failed to join game: {}", e).into());
                    // Still navigate to show the error
                    if let Some(Ok(game_id)) = create_action.value().get() {
                        navigate(&format!("/games/{}", game_id), Default::default());
                    }
                }
            }
        }
    });

    view! {
        <div class="create-game-page">
            <div class="page-header">
                <h1>"Create New Game"</h1>
            </div>

            <form on:submit=on_submit class="create-game-form">
                <div class="form-section">
                    <h3>"Game Settings"</h3>

                    <div class="form-group">
                        <label>"Number of Players"</label>
                        <div class="radio-group">
                            <label class="radio-option">
                                <input
                                    type="radio"
                                    name="player_count"
                                    value="2"
                                    checked=move || player_count.get() == 2
                                    on:change=move |_| set_player_count.set(2)
                                />
                                <span>"2 Players"</span>
                            </label>
                            <label class="radio-option">
                                <input
                                    type="radio"
                                    name="player_count"
                                    value="4"
                                    checked=move || player_count.get() == 4
                                    on:change=move |_| set_player_count.set(4)
                                />
                                <span>"4 Players"</span>
                            </label>
                        </div>
                        <small class="form-hint">"Standard two-player or four-player game"</small>
                    </div>

                    <div class="form-group">
                        <label class="checkbox-option">
                            <input
                                type="checkbox"
                                checked=use_column_rule
                                on:change=move |ev| set_use_column_rule.set(event_target_checked(&ev))
                            />
                            <span>"Use Column Rule"</span>
                        </label>
                        <small class="form-hint">
                            "When automaton priorities tie, prefer horizontal movement. Recommended for standard play."
                        </small>
                    </div>
                </div>

                {move || {
                    if let Some(Err(e)) = create_action.value().get() {
                        view! {
                            <div class="error-message">
                                {format!("Failed to create game: {}", e)}
                            </div>
                        }.into_any()
                    } else if let Some(Err(e)) = join_action.value().get() {
                        view! {
                            <div class="error-message">
                                {format!("Game created but failed to join: {}", e)}
                            </div>
                        }.into_any()
                    } else if join_action.pending().get() {
                        view! {
                            <div class="success-message">
                                "Game created! Joining..."
                            </div>
                        }.into_any()
                    } else {
                        view! {}.into_any()
                    }
                }}

                <div class="form-actions">
                    <button
                        type="submit"
                        class="button button-primary button-large"
                        disabled=move || create_action.pending().get() || join_action.pending().get()
                    >
                        {move || {
                            if join_action.pending().get() {
                                "Joining..."
                            } else if create_action.pending().get() {
                                "Creating..."
                            } else {
                                "Create Game"
                            }
                        }}
                    </button>
                    <A href="/games" attr:class="button button-secondary">
                        "Cancel"
                    </A>
                </div>
            </form>
        </div>
    }
}

#[component]
pub fn GamePage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();
    let params = use_params_map();

    let game_id = Memo::new(move |_| {
        params
            .get()
            .get("id")
            .and_then(|id| Uuid::parse_str(&id).ok())
    });

    // Initialize game state and get signal
    let app_state_for_init = app_state.clone();
    Effect::new(move |_| {
        if let Some(gid) = game_id.get() {
            app_state_for_init.ensure_game_state(gid);
        }
    });

    // Fetch initial game state (once)
    let initial_load_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        async move {
            let client = create_api_client();
            client.get_game_state_typed(gid).await
        }
    });

    // Setup: fetch initial state and connect WebSocket
    let app_state_for_setup = app_state.clone();
    Effect::new(move |_| {
        if let Some(gid) = game_id.get() {
            // Fetch initial state
            initial_load_action.dispatch(gid);

            // Connect WebSocket - the connection is kept alive by the effect!
            // When effect re-runs or component unmounts, connection drops
            let _ws = create_game_websocket(gid, &app_state_for_setup);

            // Return cleanup function
            Some(move || {
                drop(_ws);
            })
        } else {
            None
        }
    });

    // Update state signal when initial load completes
    let app_state_for_load = app_state.clone();
    Effect::new(move |_| {
        if let Some(result) = initial_load_action.value().get() {
            if let Some(gid) = game_id.get() {
                match result {
                    Ok(state) => {
                        app_state_for_load.update_game_state(gid, state);
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("Failed to load game: {}", e).into());
                    }
                }
            }
        }
    });

    // Auto-join action for when viewing a game that's waiting for players
    let auto_join_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        async move {
            let client = create_api_client();
            client.join_game(gid).await
        }
    });

    // Track if we've already attempted auto-join for this game
    let (auto_join_attempted, set_auto_join_attempted) = signal(Option::<Uuid>::None);

    // Get reactive game state signal
    let app_state_for_signal = app_state.clone();
    let game_state_signal = Memo::new(move |_| {
        game_id
            .get()
            .and_then(|gid| app_state_for_signal.get_game_signal(gid))
            .and_then(|sig| sig.get())
    });

    // Auto-join logic: join if authenticated, not already in game, and game is waiting
    let app_state_for_auto_join = app_state.clone();
    Effect::new(move |_| {
        if let Some(state) = game_state_signal.get() {
            if let Some(gid) = game_id.get() {
                let current_player_id = app_state_for_auto_join.current_player_id.get();
                let is_in_game = current_player_id
                    .and_then(|pid| state.player_ids.get(&pid))
                    .is_some();
                let is_waiting = matches!(state.lifecycle, GameLifecycle::Waiting);
                let already_attempted = auto_join_attempted.get() == Some(gid);

                // Auto-join if: authenticated, not in game, game is waiting, and haven't tried yet
                if current_player_id.is_some() && !is_in_game && is_waiting && !already_attempted {
                    set_auto_join_attempted.set(Some(gid));
                    auto_join_action.dispatch(gid);
                }
            }
        }
    });

    // Refresh after auto-join (triggers HTTP refetch for updated player list)
    Effect::new(move |_| {
        if let Some(result) = auto_join_action.value().get() {
            if let Some(gid) = game_id.get() {
                match result {
                    Ok(_) => {
                        toast.success("Joined game successfully!");
                        // Re-fetch state to get updated player list
                        initial_load_action.dispatch(gid);
                    }
                    Err(e) => {
                        toast.warning(format!("Could not join: {}. You can still spectate.", e));
                    }
                }
            }
        }
    });

    // Cleanup when leaving page
    let app_state_for_cleanup = app_state.clone();
    on_cleanup(move || {
        if let Some(gid) = game_id.get_untracked() {
            app_state_for_cleanup.remove_game_state(gid);
        }
        // WebSocket connection will be dropped by effect cleanup
    });

    view! {
        <div class="game-page">
            {move || {
                if auto_join_action.pending().get() {
                    view! {
                        <div class="loading-state">
                            <div class="spinner"></div>
                            <p>"Joining game..."</p>
                        </div>
                    }.into_any()
                } else if let Some(Err(e)) = auto_join_action.value().get() {
                    view! {
                        <div class="error-message" style="margin: 20px;">
                            {format!("Failed to join game: {}. You can still spectate.", e)}
                        </div>
                    }.into_any()
                } else {
                    view! {}.into_any()
                }
            }}

            {move || {
                if initial_load_action.pending().get() {
                    view! { <SkeletonGameBoard /> }.into_any()
                } else if let Some(state) = game_state_signal.get() {
                    let gid = game_id.get().unwrap();
                    view! {
                        <GameView game_id=gid game_state=state />
                    }.into_any()
                } else if let Some(Err(e)) = initial_load_action.value().get() {
                    view! {
                        <div class="error-state">
                            <h3>"Error loading game"</h3>
                            <p>{format!("{}", e)}</p>
                            <A href="/games" attr:class="button">
                                "Back to Games"
                            </A>
                        </div>
                    }.into_any()
                } else {
                    view! { <SkeletonGameBoard /> }.into_any()
                }
            }}
        </div>
    }
}

/// Improved game view with tabbed interface
#[component]
fn GameView(game_id: Uuid, game_state: automatafl_api_types::GameStateResponse) -> impl IntoView {
    let (active_right_tab, set_active_right_tab) = signal("chat".to_string());
    let (show_move_form, set_show_move_form) = signal(false);

    // Register keyboard shortcuts for game page
    use crate::components::use_keyboard_shortcut;

    // Ctrl+1, Ctrl+2 to switch tabs
    use_keyboard_shortcut("Ctrl+1", move || {
        set_active_right_tab.set("chat".to_string())
    });
    use_keyboard_shortcut("Ctrl+2", move || {
        set_active_right_tab.set("history".to_string())
    });

    // Ctrl+M to toggle move form
    use_keyboard_shortcut("Ctrl+M", move || set_show_move_form.update(|v| *v = !*v));

    view! {
        <div class="game-container">
            <div class="game-header">
                <GameInfo game_state=game_state.clone() />
            </div>
            <div class="game-main">
                <div class="game-left-panel">
                    {{
                        let game_state_clone = game_state.clone();
                        move || if !show_move_form.get() {
                            view! {
                                <div class="game-controls-compact">
                                    <RoundControls game_id=game_id game_state=game_state_clone.clone() />
                                    <SaveLoadControls game_id=game_id />
                                    <button
                                        class="button button-small button-secondary"
                                        on:click=move |_| set_show_move_form.set(true)
                                    >
                                        "📝 Show Move Form"
                                    </button>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="game-controls-expanded">
                                    <MoveControls game_id=game_id game_state=game_state_clone.clone() />
                                    <RoundControls game_id=game_id game_state=game_state_clone.clone() />
                                    <SaveLoadControls game_id=game_id />
                                    <button
                                        class="button button-small button-secondary"
                                        on:click=move |_| set_show_move_form.set(false)
                                    >
                                        "Hide Move Form"
                                    </button>
                                </div>
                            }.into_any()
                        }
                    }}
                </div>
                <div class="game-center">
                    <GameBoard game_id=game_id game_state=game_state.clone() />
                </div>
                <div class="game-right-panel">
                    <div class="right-panel-tabs">
                        <button
                            class=move || format!("tab {}", if active_right_tab.get() == "chat" { "active" } else { "" })
                            on:click=move |_| set_active_right_tab.set("chat".to_string())
                        >
                            "💬 Chat"
                        </button>
                        <button
                            class=move || format!("tab {}", if active_right_tab.get() == "history" { "active" } else { "" })
                            on:click=move |_| set_active_right_tab.set("history".to_string())
                        >
                            "📜 History"
                        </button>
                    </div>
                    <div class="right-panel-content">
                        <Show when=move || active_right_tab.get() == "chat">
                            <ChatPanel game_id=game_id />
                        </Show>
                        <Show when=move || active_right_tab.get() == "history">
                            <GameHistory game_id=game_id />
                        </Show>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn GameHistoryPage() -> impl IntoView {
    view! {
        <div class="game-history-page">
            <h1>"Game History"</h1>
            <div class="stub-notice">
                <p>"Individual game history pages coming soon. Use the history tab in active games."</p>
                <A href="/games" attr:class="button">
                    "Back to Games"
                </A>
            </div>
        </div>
    }
}

#[component]
pub fn SpectatePage() -> impl IntoView {
    view! {
        <div class="spectate-page">
            <h1>"Spectate Game"</h1>
            <div class="stub-notice">
                <p>"Spectator mode uses the same game view. Spectator-specific features coming soon."</p>
                <A href="/games" attr:class="button">
                    "Back to Games"
                </A>
            </div>
        </div>
    }
}
