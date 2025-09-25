use crate::{api::ApiClient, state::{AppState, MatchmakingState}};
use automatafl_api::{JoinMatchmakingRequest, MatchmakingStatusResponse};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

#[component]
pub fn MatchmakingPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let navigate = use_navigate();
    
    let (time_control, set_time_control) = create_signal("rapid".to_string());
    let (rating_range_enabled, set_rating_range_enabled) = create_signal(false);
    let (min_rating, set_min_rating) = create_signal(String::new());
    let (max_rating, set_max_rating) = create_signal(String::new());
    let (error, set_error) = create_signal(Option::<String>::None);
    
    let matchmaking_status = app_state.matchmaking_status;
    
    // Poll matchmaking status
    let poll_interval = store_value(None::<leptos::leptos_dom::helpers::IntervalHandle>);
    
    let start_matchmaking = move |_| {
        set_error.set(None);
        
        let rating_range = if rating_range_enabled.get() {
            let min = min_rating.get().parse::<i32>().ok();
            let max = max_rating.get().parse::<i32>().ok();
            
            match (min, max) {
                (Some(min), Some(max)) if min <= max => Some((min, max)),
                _ => {
                    set_error.set(Some("Invalid rating range".to_string()));
                    return;
                }
            }
        } else {
            None
        };
        
        let request = JoinMatchmakingRequest {
            time_control: Some(time_control.get()),
            rating_range,
        };
        
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.join_matchmaking(request).await {
                Ok(_) => {
                    // Start polling for status
                    let handle = leptos::leptos_dom::helpers::set_interval_with_handle(
                        move || {
                            spawn_local(async move {
                                let client = ApiClient::new(
                                    app_state.api_base_url.clone(),
                                    app_state.auth_token.get()
                                );
                                
                                if let Ok(status) = client.matchmaking_status().await {
                                    if status.in_queue {
                                        matchmaking_status.set(MatchmakingState::InQueue {
                                            estimated_wait: status.estimated_wait_time_seconds,
                                            players_in_queue: status.players_in_queue,
                                        });
                                    } else {
                                        // Check if match was found
                                        matchmaking_status.set(MatchmakingState::NotInQueue);
                                        if let Some(handle) = poll_interval.get_value() {
                                            handle.clear();
                                        }
                                    }
                                }
                            });
                        },
                        std::time::Duration::from_secs(2),
                    ).ok();
                    
                    poll_interval.set_value(handle);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
        });
    };
    
    let cancel_matchmaking = move |_| {
        // Clear polling
        if let Some(handle) = poll_interval.get_value() {
            handle.clear();
        }
        
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            let _ = client.leave_matchmaking().await;
            matchmaking_status.set(MatchmakingState::NotInQueue);
        });
    };
    
    // Navigate when match is found
    Effect::new(move |_| {
        if let MatchmakingState::MatchFound { game_id } = matchmaking_status.get() {
            navigate(&format!("/games/{}", game_id), Default::default());
        }
    });

    view! {
        <div class="matchmaking-page">
            <div class="matchmaking-container">
                <h1>"🎮 Quick Match"</h1>
                <p class="subtitle">"Find an opponent automatically based on your skill level"</p>
                
                {move || match matchmaking_status.get() {
                    MatchmakingState::NotInQueue => view! {
                        <div class="matchmaking-setup">
                            <div class="setup-section">
                                <h3>"Time Control"</h3>
                                <div class="time-control-options">
                                    <label class="radio-option">
                                        <input
                                            type="radio"
                                            name="time-control"
                                            value="blitz"
                                            checked=move || time_control.get() == "blitz"
                                            on:change=move |_| set_time_control.set("blitz".to_string())
                                        />
                                        <span class="option-label">"⚡ Blitz (5 min)"</span>
                                    </label>
                                    <label class="radio-option">
                                        <input
                                            type="radio"
                                            name="time-control"
                                            value="rapid"
                                            checked=move || time_control.get() == "rapid"
                                            on:change=move |_| set_time_control.set("rapid".to_string())
                                        />
                                        <span class="option-label">"🏃 Rapid (10 min)"</span>
                                    </label>
                                    <label class="radio-option">
                                        <input
                                            type="radio"
                                            name="time-control"
                                            value="classical"
                                            checked=move || time_control.get() == "classical"
                                            on:change=move |_| set_time_control.set("classical".to_string())
                                        />
                                        <span class="option-label">"♟️ Classical (30 min)"</span>
                                    </label>
                                </div>
                            </div>
                            
                            <div class="setup-section">
                                <label class="checkbox-option">
                                    <input
                                        type="checkbox"
                                        checked=rating_range_enabled
                                        on:change=move |ev| set_rating_range_enabled.set(event_target_checked(&ev))
                                    />
                                    <span>"Set rating range for opponents"</span>
                                </label>
                                
                                <Show when=move || rating_range_enabled.get()>
                                    <div class="rating-range-inputs">
                                        <input
                                            type="number"
                                            class="form-input"
                                            placeholder="Min rating"
                                            value=min_rating
                                            on:input=move |ev| set_min_rating.set(event_target_value(&ev))
                                        />
                                        <span class="range-separator">"to"</span>
                                        <input
                                            type="number"
                                            class="form-input"
                                            placeholder="Max rating"
                                            value=max_rating
                                            on:input=move |ev| set_max_rating.set(event_target_value(&ev))
                                        />
                                    </div>
                                </Show>
                            </div>
                            
                            <Show when=move || error.get().is_some()>
                                <div class="error-message">
                                    {move || error.get().unwrap_or_default()}
                                </div>
                            </Show>
                            
                            <button
                                class="button button-primary button-large"
                                on:click=start_matchmaking
                            >
                                "Find Match"
                            </button>
                        </div>
                    },
                    MatchmakingState::InQueue { estimated_wait, players_in_queue } => view! {
                        <div class="matchmaking-queue">
                            <div class="queue-animation">
                                <div class="spinner-large"></div>
                            </div>
                            
                            <h2>"Searching for opponent..."</h2>
                            
                            <div class="queue-info">
                                <div class="info-item">
                                    <span class="info-label">"Players in queue:"</span>
                                    <span class="info-value">{players_in_queue}</span>
                                </div>
                                
                                <Show when=move || estimated_wait.is_some()>
                                    <div class="info-item">
                                        <span class="info-label">"Estimated wait:"</span>
                                        <span class="info-value">
                                            {format!("{} seconds", estimated_wait.unwrap_or(0))}
                                        </span>
                                    </div>
                                </Show>
                                
                                <div class="info-item">
                                    <span class="info-label">"Time control:"</span>
                                    <span class="info-value">
                                        {match time_control.get().as_str() {
                                            "blitz" => "Blitz (5 min)",
                                            "rapid" => "Rapid (10 min)",
                                            "classical" => "Classical (30 min)",
                                            _ => "Unknown",
                                        }}
                                    </span>
                                </div>
                            </div>
                            
                            <button
                                class="button button-secondary"
                                on:click=cancel_matchmaking
                            >
                                "Cancel"
                            </button>
                        </div>
                    },
                    MatchmakingState::MatchFound { .. } => view! {
                        <div class="match-found">
                            <div class="success-icon">"✅"</div>
                            <h2>"Match Found!"</h2>
                            <p>"Redirecting to game..."</p>
                        </div>
                    },
                }}
            </div>
        </div>
    }
}
