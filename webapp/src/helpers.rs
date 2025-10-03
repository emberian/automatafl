// Helper utilities to reduce code duplication across components
use crate::{api::ApiClient, components::use_toast, state::AppState};
use leptos::prelude::*;
use uuid::Uuid;

/// Create an API client with the current session
pub fn create_api_client() -> ApiClient {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    app_state.get_api_client()
}

/// Helper to create a resource that uses the API client and reacts to a trigger.
/// This is the core helper that all other resource helpers should use.
pub fn create_api_resource_with_trigger<T, F, Fut, V>(
    trigger: impl Fn() -> V + 'static,
    fetch_fn: F,
) -> LocalResource<Result<T, automatafl_backend_client::ClientError>>
where
    T: 'static,
    V: 'static,
    F: Fn(ApiClient) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, automatafl_backend_client::ClientError>> + 'static,
{
    let app_state = use_context::<AppState>().expect("AppState should be provided");

    LocalResource::new(move || {
        trigger(); // React to the trigger
        let client = app_state.get_api_client();
        fetch_fn(client)
    })
}

/// Helper to create a resource that uses the API client (no trigger)
pub fn create_api_resource<T, F, Fut>(
    fetch_fn: F,
) -> LocalResource<Result<T, automatafl_backend_client::ClientError>>
where
    T: 'static,
    F: Fn(ApiClient) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, automatafl_backend_client::ClientError>> + 'static,
{
    create_api_resource_with_trigger(|| (), fetch_fn)
}

/// Helper to create a refreshable resource (with manual refresh trigger)
pub fn create_refreshable_resource<T, F, Fut>(
    refresh_trigger: ReadSignal<u32>,
    fetch_fn: F,
) -> LocalResource<Result<T, automatafl_backend_client::ClientError>>
where
    T: 'static,
    F: Fn(ApiClient) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, automatafl_backend_client::ClientError>> + 'static,
{
    create_api_resource_with_trigger(move || refresh_trigger.get(), fetch_fn)
}

/// Helper to create a game-aware resource that refreshes on WebSocket state changes
/// Note: This is now deprecated - prefer subscribing directly to game state signals
pub fn create_game_resource<T, F, Fut>(
    game_id: Uuid,
    fetch_fn: F,
) -> LocalResource<Result<T, automatafl_backend_client::ClientError>>
where
    T: 'static,
    F: Fn(ApiClient) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, automatafl_backend_client::ClientError>> + 'static,
{
    let app_state = use_context::<AppState>().expect("AppState should be provided");

    create_api_resource_with_trigger(
        move || {
            // React to game state signal changes (full state updates)
            app_state.get_game_signal(game_id).map(|sig| sig.get())
        },
        fetch_fn,
    )
}

/// Helper to handle action results with toast notifications
pub fn handle_action_result<T: Clone + 'static>(
    action: Action<impl Clone + 'static, Result<T, automatafl_backend_client::ClientError>>,
    on_success: impl Fn(T) + 'static,
    success_message: Option<String>,
    error_prefix: Option<String>,
) {
    let toast = use_toast();

    Effect::new(move |_| {
        if let Some(result) = action.value().get() {
            match result {
                Ok(value) => {
                    if let Some(msg) = success_message.clone() {
                        toast.success(msg);
                    }
                    on_success(value);
                }
                Err(e) => {
                    let prefix = error_prefix
                        .clone()
                        .unwrap_or_else(|| "Operation failed".to_string());
                    toast.error(format!("{}: {}", prefix, e));
                }
            }
        }
    });
}

/// Simple toast notification for action results
/// Use this in an Effect that watches action.value()
pub fn toast_on_result<T>(
    result: Option<Result<T, automatafl_backend_client::ClientError>>,
    success_message: impl Into<String>,
    error_prefix: impl Into<String>,
) {
    let toast = use_toast();

    if let Some(res) = result {
        match res {
            Ok(_) => toast.success(success_message.into()),
            Err(e) => toast.error(format!("{}: {}", error_prefix.into(), e)),
        }
    }
}

/// Debounce a callback (useful for search inputs, chat, etc.)
pub fn use_debounced_callback<F>(callback: F, delay_ms: u32) -> impl Fn() + Clone
where
    F: Fn() + Clone + 'static,
{
    use crate::utils::Debouncer;
    use std::rc::Rc;

    let debouncer = Rc::new(Debouncer::new(delay_ms));

    move || {
        let callback_clone = callback.clone();
        let debouncer_clone = debouncer.clone();
        debouncer_clone.debounce(move || callback_clone());
    }
}

/// Format timestamp as human-readable string
pub fn format_timestamp(timestamp: u64, format: &str) -> String {
    chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .map(|dt| dt.format(format).to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Format timestamp as relative time ("2 minutes ago", "just now", etc.)
pub fn format_relative_time(timestamp: u64) -> String {
    let now = js_sys::Date::now() as u64 / 1000;
    let diff = now.saturating_sub(timestamp);

    match diff {
        0..=60 => "just now".to_string(),
        61..=3600 => format!(
            "{} minute{} ago",
            diff / 60,
            if diff / 60 == 1 { "" } else { "s" }
        ),
        3601..=86400 => format!(
            "{} hour{} ago",
            diff / 3600,
            if diff / 3600 == 1 { "" } else { "s" }
        ),
        _ => format!(
            "{} day{} ago",
            diff / 86400,
            if diff / 86400 == 1 { "" } else { "s" }
        ),
    }
}

/// Truncate UUID for display
pub fn truncate_uuid(uuid: Uuid, chars: usize) -> String {
    uuid.to_string().chars().take(chars).collect::<String>() + "..."
}

/// Check if user is authenticated (common pattern)
pub fn use_auth_check() -> bool {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    app_state.is_authenticated()
}

/// Get current player ID (common pattern)
pub fn use_current_player_id() -> Option<Uuid> {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    app_state.current_player_id.get()
}

/// Helper to create confirmation modal with common patterns
pub fn use_confirm_delete(
    item_name: impl Into<String>,
    on_confirm: impl Fn() + 'static + Send + Sync,
) {
    let modal = crate::components::use_modal();
    modal.confirm_danger(
        format!("Delete {}?", item_name.into()),
        "This action cannot be undone.",
        on_confirm,
    );
}
