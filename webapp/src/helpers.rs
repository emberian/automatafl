// Helper utilities to reduce code duplication across components
//
// BEST PRACTICES:
// 1. For real-time game data (moves, chat, events): Read from AppState signals, NOT HTTP resources
// 2. For static/snapshot data (game lists, snapshots): Use HTTP resources with refresh triggers
// 3. Resources automatically track any signals read inside them - no manual trigger() needed
// 4. Prefer direct signal subscriptions over LocalResource when data is already in AppState

use crate::{api::ApiClient, components::use_toast, state::AppState};
use leptos::prelude::*;

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
        // CRITICAL: Use .with() for a single reactive read instead of separate .get() calls
        // This prevents double-subscription and reduces re-render overhead
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