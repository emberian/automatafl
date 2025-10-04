// Helper utilities to reduce code duplication across components
//
// BEST PRACTICES:
// 1. For real-time game data (moves, chat, events): Read from AppState signals, NOT HTTP resources
// 2. For static/snapshot data (game lists, snapshots): Use HTTP resources with refresh triggers
// 3. Resources automatically track any signals read inside them - no manual trigger() needed
// 4. Prefer direct signal subscriptions over LocalResource when data is already in AppState

use crate::{api::ApiClient, components::use_toast, state::AppState};
use leptos::prelude::*;
use uuid::Uuid;

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

/// Creates a Leptos Action that automatically displays success/error toasts.
/// This is more ergonomic than creating an action and then calling handle_action_result.
pub fn use_toast_action<I, O, E, F, Fut>(
    action_fn: F,
    success_message: Option<String>,
    error_prefix: Option<String>,
) -> Action<I, Result<O, E>>
where
    I: Clone + 'static,
    O: Clone + 'static,
    E: std::fmt::Display + 'static,
    F: Fn(&I) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<O, E>> + 'static,
{
    let action = Action::new_local(action_fn);
    let toast = use_toast();
    let value_signal = action.value();

    Effect::new(move |_| {
        // Use .version() to track action completion
        action.version().track();

        value_signal.with(|maybe_result| {
            if let Some(result) = maybe_result {
                match result {
                    Ok(_) => {
                        if let Some(msg) = success_message.clone() {
                            toast.success(msg);
                        }
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
    });

    action
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
    app_state.is_authenticated.get()
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
