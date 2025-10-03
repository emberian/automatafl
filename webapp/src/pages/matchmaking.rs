use crate::{api::ApiClient, state::AppState};
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn MatchmakingPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (player_count, set_player_count) = signal(2u8);
    let (use_column_rule, set_use_column_rule) = signal(true);
    let (error_message, set_error_message) = signal(Option::<String>::None);
    
    // Poll matchmaking status
    let api_base_url = app_state.api_base_url.clone();
    let (poll_trigger, set_poll_trigger) = signal(0u32);
    
    let status_resource = LocalResource::new(move || {
        let _ = poll_trigger.get();
        let api_base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(api_base_url);
            client.get_matchmaking_status().await
        }
    });
    
    // Set up polling interval when in queue
    Effect::new(move |_| {
        if let Some(Ok(status)) = status_resource.get() {
            if status.in_queue {
                set_timeout(
                    move || set_poll_trigger.update(|n| *n += 1),
                    std::time::Duration::from_secs(2)
                );
            }
        }
    });
    
    // Join matchmaking action
    let api_base_url_for_join = app_state.api_base_url.clone();
    let join_action = Action::new_local(move |(pc, ucr): &(u8, bool)| {
        let pc = *pc;
        let ucr = *ucr;
        let base_url = api_base_url_for_join.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.join_matchmaking(pc, ucr).await
        }
    });
    
    // Leave matchmaking action
    let api_base_url_for_leave = app_state.api_base_url.clone();
    let leave_action = Action::new_local(move |_: &()| {
        let base_url = api_base_url_for_leave.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.leave_matchmaking().await
        }
    });
    
    // Handle join result
    Effect::new(move |_| {
        if let Some(result) = join_action.value().get() {
            match result {
                Ok(_) => {
                    set_error_message.set(None);
                    set_poll_trigger.update(|n| *n += 1);
                }
                Err(e) => {
                    set_error_message.set(Some(format!("Failed to join queue: {}", e)));
                }
            }
        }
    });
    
    // Handle leave result
    Effect::new(move |_| {
        if let Some(result) = leave_action.value().get() {
            match result {
                Ok(_) => {
                    set_error_message.set(None);
                    set_poll_trigger.update(|n| *n += 1);
                }
                Err(e) => {
                    set_error_message.set(Some(format!("Failed to leave queue: {}", e)));
                }
            }
        }
    });
    
    let on_join = move |_| {
        join_action.dispatch((player_count.get(), use_column_rule.get()));
    };
    
    let on_leave = move |_| {
        leave_action.dispatch(());
    };

    view! {
        <div class="matchmaking-page">
            <div class="page-header">
                <h1>"Quick Match"</h1>
            </div>
            
            {move || error_message.get().map(|msg| view! {
                <div class="error-message">{msg}</div>
            })}
            
            <Suspense fallback=move || view! {
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading matchmaking status..."</p>
                </div>
            }>
                {move || {
                    status_resource.get().map(|result| {
                        match result {
                            Ok(status) => {
                                if !status.in_queue {
                                    view! {
                                        <div class="matchmaking-join-form">
                                            <div class="card">
                                                <h2>"Join Matchmaking Queue"</h2>
                                                
                                                <div class="form-group">
                                                    <label>"Number of Players"</label>
                                                    <div class="radio-group">
                                                        <label class="radio-option">
                                                            <input
                                                                type="radio"
                                                                name="player_count"
                                                                checked=move || player_count.get() == 2
                                                                on:change=move |_| set_player_count.set(2)
                                                            />
                                                            <span>"2 Players"</span>
                                                        </label>
                                                        <label class="radio-option">
                                                            <input
                                                                type="radio"
                                                                name="player_count"
                                                                checked=move || player_count.get() == 4
                                                                on:change=move |_| set_player_count.set(4)
                                                            />
                                                            <span>"4 Players"</span>
                                                        </label>
                                                    </div>
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
                                                </div>
                                                
                                                <button 
                                                    class="button button-primary button-large"
                                                    on:click=on_join
                                                    disabled=move || join_action.pending().get()
                                                >
                                                    {move || if join_action.pending().get() { "Joining..." } else { "Join Queue" }}
                                                </button>
                                            </div>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="matchmaking-waiting">
                                            <div class="card">
                                                <h2>"⏳ Searching for Match..."</h2>
                                                <div class="waiting-indicator">
                                                    <div class="spinner"></div>
                                                    <p>"Looking for players..."</p>
                                                </div>
                                                
                                                <div class="queue-info">
                                                    {if let Some(queued_at) = status.queued_at {
                                                        view! {
                                                            <p>"Joined queue at: " {
                                                                chrono::DateTime::from_timestamp(queued_at as i64, 0)
                                                                    .map(|dt| dt.format("%H:%M:%S").to_string())
                                                                    .unwrap_or_else(|| "Unknown".to_string())
                                                            }</p>
                                                        }.into_any()
                                                    } else {
                                                        view! {}.into_any()
                                                    }}
                                                    {if let Some(wait_time) = status.estimated_wait_time {
                                                        view! {
                                                            <p>"Estimated wait: " {wait_time} " seconds"</p>
                                                        }.into_any()
                                                    } else {
                                                        view! {}.into_any()
                                                    }}
                                                </div>
                                                
                                                <button 
                                                    class="button button-danger"
                                                    on:click=on_leave
                                                    disabled=move || leave_action.pending().get()
                                                >
                                                    {move || if leave_action.pending().get() { "Leaving..." } else { "Leave Queue" }}
                                                </button>
                                            </div>
                                        </div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! {
                                <div class="error-state">
                                    <h3>"Error loading matchmaking status"</h3>
                                    <p>{format!("{}", e)}</p>
                                    <A href="/games" attr:class="button">
                                        "Back to Games"
                                    </A>
                                </div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}
