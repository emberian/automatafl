// ChatPanel component - displays game chat
use crate::{api::ApiClient, state::AppState, components::use_toast, utils::RateLimiter};
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn ChatPanel(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();
    
    // Rate limiter: 5 messages per 10 seconds
    let rate_limiter = RateLimiter::new(5, 10000);
    
    let (message_input, set_message_input) = signal(String::new());
    
    let api_base_url = app_state.api_base_url.clone();
    let app_state_for_resource = app_state.clone();
    let messages_resource = LocalResource::new(
        move || {
            // Track game refresh trigger to make resource reactive to WebSocket events
            let _trigger = app_state_for_resource.get_game_refresh_trigger(game_id);
            let api_base_url = api_base_url.clone();
            async move {
                let client = ApiClient::new(api_base_url);
                client.get_chat(game_id).await
            }
        }
    );
    
    let api_base_url_for_action = app_state.api_base_url.clone();
    let send_message_action = Action::new_local(move |(gid, msg): &(Uuid, String)| {
        let gid = *gid;
        let msg = msg.clone();
        let base_url = api_base_url_for_action.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.send_chat(gid, msg).await
        }
    });

    let toast_clone = toast.clone();
    let send_message = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        let msg = message_input.get();
        if msg.trim().is_empty() {
            return;
        }

        // Check rate limit
        if !rate_limiter.allow() {
            let remaining = rate_limiter.remaining();
            toast_clone.warning(format!("Slow down! You can send {} more messages in a few seconds.", remaining));
            return;
        }

        send_message_action.dispatch((game_id, msg));
    };

    let toast_clone2 = toast.clone();
    Effect::new(move |_| {
        if let Some(result) = send_message_action.value().get() {
            match result {
                Ok(_) => {
                    set_message_input.set(String::new());
                    // Chat will refresh via WebSocket CHAT event triggering game refresh
                }
                Err(e) => {
                    toast_clone2.error(format!("Failed to send message: {}", e));
                }
            }
        }
    });
    
    view! {
        <div class="chat-panel">
            <h3>"Chat"</h3>
            
            <div class="chat-messages">
                <Suspense fallback=move || view! {
                    <div class="loading">Loading messages...</div>
                }>
                    {move || {
                        messages_resource.get().map(|result| {
                            match result {
                                Ok(messages) => {
                                    if messages.is_empty() {
                                        view! {
                                            <div class="chat-empty">
                                                <p>"No messages yet"</p>
                                            </div>
                                        }.into_any()
                                    } else {
                                        messages.into_iter().map(|message| {
                                            view! {
                                                <div class="chat-message">
                                                    <div class="message-header">
                                                        <span class="message-author">{message.displayname.clone()}</span>
                                                        <span class="message-time">
                                                            {chrono::DateTime::from_timestamp(message.timestamp as i64, 0)
                                                                .map(|dt| dt.format("%H:%M:%S").to_string())
                                                                .unwrap_or_else(|| "".to_string())}
                                                        </span>
                                                    </div>
                                                    <div class="message-content">{message.message.clone()}</div>
                                                </div>
                                            }
                                        }).collect_view().into_any()
                                    }
                                }
                                Err(e) => view! {
                                    <div class="chat-error">
                                        <p>"Error loading messages: " {format!("{}", e)}</p>
                                    </div>
                                }.into_any()
                            }
                        })
                    }}
                </Suspense>
            </div>
            
            <form on:submit=send_message class="chat-input-form">
                <input
                    type="text"
                    class="chat-input"
                    placeholder="Type a message..."
                    on:input=move |ev| set_message_input.set(event_target_value(&ev))
                    prop:value=message_input
                />
                <button
                    type="submit"
                    class="button button-small"
                    disabled=move || send_message_action.pending().get() || message_input.get().trim().is_empty()
                >
                    {move || if send_message_action.pending().get() { "..." } else { "Send" }}
                </button>
            </form>
        </div>
    }
}
