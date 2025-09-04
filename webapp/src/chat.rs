used crate::api::ApiClient;
use automatafl_api::ChatMessage;
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn GameChat(
    game_id: Uuid,
    api: ApiClient,
    current_user_id: Option<Uuid>,
) -> impl IntoView {
    let (messages, set_messages) = create_signal(Vec::<ChatMessage>::new());
    let (input_text, set_input_text) = create_signal(String::new());
    let (loading, set_loading) = create_signal(false);
    let (error, set_error) = create_signal(None::<String>);
    
    // Ref for auto-scrolling
    let messages_container_ref = create_node_ref::<leptos::html::Div>();

    // Load initial messages
    let api_clone = api.clone();
    create_effect(move |_| {
        spawn_local(async move {
            match api_clone.get_messages(game_id).await {
                Ok(msgs) => {
                    set_messages.set(msgs);
                    // Scroll to bottom after messages load
                    if let Some(container) = messages_container_ref.get() {
                        container.set_scroll_top(container.scroll_height());
                    }
                }
                Err(e) => {
                    leptos::logging::error!("Failed to load messages: {}", e);
                }
            }
        });
    });

    // Auto-scroll when new messages arrive
    create_effect(move |_| {
        messages.get();
        if let Some(container) = messages_container_ref.get() {
            // Small delay to ensure DOM is updated
            gloo_timers::callback::Timeout::new(10, move || {
                if let Some(container) = messages_container_ref.get() {
                    container.set_scroll_top(container.scroll_height());
                }
            }).forget();
        }
    });

    let send_message = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        
        let text = input_text.get();
        if text.trim().is_empty() {
            return;
        }

        let api = api.clone();
        spawn_local(async move {
            set_loading.set(true);
            set_error.set(None);
            
            match api.send_message(game_id, text).await {
                Ok(new_message) => {
                    set_messages.update(|msgs| msgs.push(new_message));
                    set_input_text.set(String::new());
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            
            set_loading.set(false);
        });
    };

    // Helper to add a new message from WebSocket
    let add_message = move |msg: ChatMessage| {
        set_messages.update(|msgs| {
            // Check if message already exists (by ID)
            if !msgs.iter().any(|m| m.id == msg.id) {
                msgs.push(msg);
            }
        });
    };

    // Provide the add_message function via context for WebSocket handler
    provide_context(add_message);

    view! {
        <div class="chat-tab">
            <h3>"Game Chat"</h3>
            
            <div class="chat-messages" node_ref=messages_container_ref>
                {move || if messages.get().is_empty() {
                    view! {
                        <div class="chat-empty">
                            <p>"No messages yet. Start the conversation!"</p>
                        </div>
                    }.into_view()
                } else {
                    messages.get().into_iter().map(|msg| {
                        let is_own_message = current_user_id.map(|id| id == msg.user_id).unwrap_or(false);
                        let time = msg.created_at.format("%H:%M").to_string();
                        
                        view! {
                            <div class={if is_own_message { "chat-message own-message" } else { "chat-message" }}>
                                <span class="chat-player">{format!("Player {}", if msg.user_id == game_id { "System" } else { "User" })}</span>
                                <span class="chat-text">{&msg.message}</span>
                                <span class="chat-time">{time}</span>
                            </div>
                        }
                    }).collect_view()
                }}
            </div>

            {move || error.get().map(|e| view! {
                <div class="alert alert-error">{e}</div>
            })}

            <form class="chat-input" on:submit=send_message>
                <input
                    type="text"
                    placeholder="Type a message..."
                    value=input_text
                    on:input=move |ev| set_input_text.set(event_target_value(&ev))
                    disabled=loading
                />
                <button
                    type="submit"
                    class="btn btn-primary"
                    disabled=move || loading.get() || input_text.get().trim().is_empty()
                >
                    {move || if loading.get() { "..." } else { "Send" }}
                </button>
            </form>
        </div>
    }
}
