// ChatPanel component - displays game chat
use crate::{components::use_toast, state::AppState, utils::RateLimiter};
use leptos::html;
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn ChatPanel(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();

    // Rate limiter: 5 messages per 10 seconds
    let rate_limiter = RateLimiter::new(5, 10000);

    let (message_input, set_message_input) = signal(String::new());

    // Use NodeRef for DOM access instead of StoredValue
    let chat_messages_ref = NodeRef::<html::Div>::new();

    // Get reactive chat signal - updates automatically from WebSocket!
    let chat_signal = app_state.get_chat_signal(game_id);

    let messages_memo = Memo::new(move |_| chat_signal.and_then(|sig| Some(sig.get())));

    let app_state_for_send = app_state.clone();
    let send_message_action = Action::new_local(move |(gid, msg): &(Uuid, String)| {
        let app_state = app_state_for_send.clone();
        let gid = *gid;
        let msg = msg.clone();
        async move {
            let client = app_state.get_api_client();
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
            toast_clone.warning(format!(
                "Slow down! You can send {} more messages in a few seconds.",
                remaining
            ));
            return;
        }

        send_message_action.dispatch((game_id, msg));
    };

    let toast_clone2 = toast.clone();
    Effect::new(move |_| {
        send_message_action.value().with(|result| {
            if let Some(result) = result {
                match result {
                    Ok(_) => {
                        set_message_input.set(String::new());
                        // Chat will refresh via WebSocket CHAT event triggering game refresh
                        // Scroll to bottom after sending
                        if let Some(elem) = chat_messages_ref.get() {
                            let _ = elem.set_scroll_top(elem.scroll_height());
                        }
                    }
                    Err(e) => {
                        toast_clone2.error(format!("Failed to send message: {}", e));
                    }
                }
            }
        });
    });

    // Auto-scroll to bottom when new messages arrive
    // CRITICAL: Use previous value to detect actual changes, not just any get()
    Effect::new(move |prev_len: Option<usize>| {
        let current_len = messages_memo.get().map(|msgs| msgs.len()).unwrap_or(0);

        // Only scroll if message count changed (new message arrived)
        if prev_len.map(|p| p != current_len).unwrap_or(true) {
            gloo_timers::callback::Timeout::new(100, move || {
                if let Some(elem) = chat_messages_ref.get() {
                    let _ = elem.set_scroll_top(elem.scroll_height());
                }
            })
            .forget();
        }

        current_len
    });

    view! {
        <div class="chat-panel">
            <h3>"Chat"</h3>

            <div
                class="chat-messages"
                node_ref=chat_messages_ref
            >
                {move || {
                    match messages_memo.get() {
                        Some(messages) => {
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
                        None => view! {
                            <div class="loading">Loading messages...</div>
                        }.into_any()
                    }
                }}
            </div>

            <form on:submit=send_message class="chat-input-form">
                <input
                    type="text"
                    class="chat-input"
                    placeholder="Type a message..."
                    prop:value=message_input
                    on:input=move |ev| set_message_input.set(event_target_value(&ev))
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
