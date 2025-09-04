use crate::{api::ApiClient, state::AppState};
use automatafl_api::ChatMessage;
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn ChatPanel(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (messages, set_messages) = create_signal(Vec::<ChatMessage>::new());
    let (message_input, set_message_input) = create_signal(String::new());
    let (sending, set_sending) = create_signal(false);
    let (error, set_error) = create_signal(Option::<String>::None);
    
    // Load initial messages
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            if let Ok(msgs) = client.get_messages(game_id).await {
                set_messages(msgs);
            }
        });
    });
    
    let send_message = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        
        let msg = message_input.get();
        if msg.trim().is_empty() {
            return;
        }
        
        set_sending(true);
        set_error(None);
        
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.send_message(game_id, msg).await {
                Ok(new_msg) => {
                    set_messages.update(|msgs| msgs.push(new_msg));
                    set_message_input(String::new());
                    set_sending(false);
                }
                Err(e) => {
                    set_error(Some(e));
                    set_sending(false);
                }
            }
        });
    };

    view! {
        <div class="chat-panel">
            <h3>"Game Chat"</h3>
            
            <div class="chat-messages" id="chat-messages">
                <For
                    each=move || messages.get()
                    key=|msg| msg.id
                    let:msg
                >
                    <ChatMessageView message=msg />
                </For>
                
                <Show when=move || messages.get().is_empty()>
                    <div class="chat-empty">
                        <p>"No messages yet. Say hello!"</p>
                    </div>
                </Show>
            </div>
            
            <Show when=move || error.get().is_some()>
                <div class="chat-error">
                    {move || error.get().unwrap_or_default()}
                </div>
            </Show>
            
            <form class="chat-input-form" on:submit=send_message>
                <input
                    type="text"
                    class="chat-input"
                    placeholder="Type a message..."
                    value=message_input
                    on:input=move |ev| set_message_input(event_target_value(&ev))
                    disabled=sending
                />
                <button
                    type="submit"
                    class="chat-send-button"
                    disabled=move || sending.get() || message_input.get().trim().is_empty()
                >
                    {move || if sending.get() { "..." } else { "Send" }}
                </button>
            </form>
        </div>
    }
}

#[component]
fn ChatMessageView(message: ChatMessage) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let current_user = app_state.current_user;
    
    let is_own_message = move || {
        current_user.get()
            .map(|user| user.id == message.user_id)
            .unwrap_or(false)
    };
    
    let time_str = message.created_at.format("%H:%M").to_string();
    
    view! {
        <div
            class="chat-message"
            class:own-message=is_own_message
        >
            <span class="chat-time">{time_str}</span>
            <span class="chat-text">{message.message}</span>
        </div>
    }
}
