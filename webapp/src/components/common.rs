// Common UI components used across the webapp

#![allow(dead_code)] // Many props are part of the component API but not always used

use leptos::prelude::*;

/// Standard loading indicator
#[component]
pub fn LoadingSpinner(#[prop(optional)] message: Option<&'static str>) -> impl IntoView {
    view! {
        <div class="loading-state">
            <div class="spinner"></div>
            {if let Some(msg) = message {
                view! { <p>{msg}</p> }.into_any()
            } else {
                view! {}.into_any()
            }}
        </div>
    }
}

/// Skeleton loader for list items
#[component]
pub fn SkeletonList(#[prop(default = 3)] count: usize) -> impl IntoView {
    view! {
        <div class="skeleton-list">
            {(0..count).map(|_| {
                view! { <SkeletonListItem /> }
            }).collect_view()}
        </div>
    }
}

#[component]
fn SkeletonListItem() -> impl IntoView {
    view! {
        <div class="skeleton-item">
            <div class="skeleton-avatar"></div>
            <div class="skeleton-content">
                <div class="skeleton-line skeleton-title"></div>
                <div class="skeleton-line skeleton-text"></div>
            </div>
        </div>
    }
}

/// Skeleton loader for cards
#[component]
pub fn SkeletonCard() -> impl IntoView {
    view! {
        <div class="skeleton-card">
            <div class="skeleton-image"></div>
            <div class="skeleton-card-body">
                <div class="skeleton-line skeleton-title"></div>
                <div class="skeleton-line skeleton-text"></div>
                <div class="skeleton-line skeleton-text short"></div>
            </div>
        </div>
    }
}

/// Skeleton loader for game board
#[component]
pub fn SkeletonGameBoard() -> impl IntoView {
    view! {
        <div class="skeleton-game-board">
            <div class="skeleton-board-grid"></div>
        </div>
    }
}

/// Skeleton loader for table
#[component]
pub fn SkeletonTable(
    #[prop(default = 5)] rows: usize,
    #[prop(default = 4)] columns: usize,
) -> impl IntoView {
    view! {
        <div class="skeleton-table">
            <div class="skeleton-table-header">
                {(0..columns).map(|_| {
                    view! { <div class="skeleton-line skeleton-th"></div> }
                }).collect_view()}
            </div>
            {(0..rows).map(|_| {
                view! {
                    <div class="skeleton-table-row">
                        {(0..columns).map(|_| {
                            view! { <div class="skeleton-line skeleton-td"></div> }
                        }).collect_view()}
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

/// Standard error display
#[component]
pub fn ErrorDisplay(
    message: String,
    #[prop(optional)] retry: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        <div class="error-state">
            <div class="error-icon">"⚠️"</div>
            <p class="error-message">{message}</p>
            {if let Some(retry_callback) = retry {
                view! {
                    <button
                        class="button button-primary"
                        on:click=move |_| retry_callback.run(())
                    >
                        "Retry"
                    </button>
                }.into_any()
            } else {
                view! {}.into_any()
            }}
        </div>
    }
}

/// Empty state display
#[component]
pub fn EmptyState(
    icon: &'static str,
    title: &'static str,
    message: &'static str,
    #[prop(optional)] action: Option<(&'static str, Callback<()>)>,
) -> impl IntoView {
    view! {
        <div class="empty-state">
            <div class="empty-icon">{icon}</div>
            <h3>{title}</h3>
            <p>{message}</p>
            {if let Some((label, callback)) = action {
                view! {
                    <button
                        class="button button-primary"
                        on:click=move |_| callback.run(())
                    >
                        {label}
                    </button>
                }.into_any()
            } else {
                view! {}.into_any()
            }}
        </div>
    }
}

/// Status badge component
#[component]
pub fn StatusBadge(status: &'static str, variant: StatusVariant) -> impl IntoView {
    let class = match variant {
        StatusVariant::Success => "status-badge status-success",
        StatusVariant::Warning => "status-badge status-warning",
        StatusVariant::Error => "status-badge status-error",
        StatusVariant::Info => "status-badge status-info",
        StatusVariant::Neutral => "status-badge status-neutral",
    };

    view! {
        <span class=class>{status}</span>
    }
}

#[derive(Clone, Copy)]
pub enum StatusVariant {
    Success,
    Warning,
    Error,
    Info,
    Neutral,
}

/// Progress bar component
#[component]
pub fn ProgressBar(
    current: usize,
    total: usize,
    #[prop(optional)] label: Option<String>,
) -> impl IntoView {
    let percentage = if total > 0 {
        (current as f32 / total as f32 * 100.0).min(100.0)
    } else {
        0.0
    };

    view! {
        <div class="progress-bar-component">
            {if let Some(label_text) = label {
                view! {
                    <div class="progress-label">{label_text}</div>
                }.into_any()
            } else {
                view! {}.into_any()
            }}
            <div class="progress-bar-container">
                <div
                    class="progress-bar-fill"
                    style=format!("width: {}%", percentage)
                />
            </div>
            <div class="progress-text">
                {current}" / "{total}
            </div>
        </div>
    }
}

/// Confirmation dialog (uses browser's confirm for now)
pub fn confirm_action(message: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
}

/// Connection status indicator (shows active game's connection status)
#[component]
pub fn ConnectionStatus() -> impl IntoView {
    use crate::state::AppState;

    let app_state = use_context::<AppState>().expect("AppState should be provided");

    view! {
        <div class="connection-status">
            {move || {
                // Get active game ID (subscribes to active_game_id changes only)
                let active_game_id = app_state.active_game_id.get();

                active_game_id.and_then(|game_id| {
                    // Get the websocket signal for this specific game ONCE
                    // CRITICAL: Use get_untracked to avoid subscribing to the HashMap
                    app_state.get_websocket_connected_signal(game_id).map(|ws_signal| {
                        // Create a derived view that subscribes to websocket status ONLY
                        view! {
                            {move || {
                                let connected = ws_signal.get();

                                if connected {
                                    view! {
                                        <div class="status-indicator status-connected" title="Connected to server">
                                            <span class="status-dot"></span>
                                            <span class="status-text">"Live"</span>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="status-indicator status-disconnected" title="Reconnecting...">
                                            <span class="status-dot pulsing"></span>
                                            <span class="status-text">"Reconnecting..."</span>
                                        </div>
                                    }.into_any()
                                }
                            }}
                        }
                    })
                })
            }}
        </div>
    }
}

/// Simple error display component with retry
#[component]
pub fn ErrorWithRetry(message: String, on_retry: Callback<()>) -> impl IntoView {
    view! {
        <div class="error-state">
            <div class="error-icon">"⚠️"</div>
            <h3>"Something went wrong"</h3>
            <p class="error-message">{message}</p>
            <button
                class="button button-primary"
                on:click=move |_| on_retry.run(())
            >
                "Retry"
            </button>
        </div>
    }
}

/// Network status detector
#[component]
pub fn NetworkStatus() -> impl IntoView {
    use leptos::prelude::window_event_listener;

    let (is_online, set_is_online) = signal(true);

    // Use window_event_listener for automatic cleanup when component unmounts
    // CRITICAL: Don't clone signals - capture them directly in closures
    window_event_listener(leptos::ev::online, move |_| {
        set_is_online.set(true);
        web_sys::console::log_1(&"📡 Network connection restored".into());
    });

    window_event_listener(leptos::ev::offline, move |_| {
        set_is_online.set(false);
        web_sys::console::warn_1(&"📡 Network connection lost".into());
    });

    view! {
        <Show when=move || !is_online.get()>
            <div class="network-status-banner">
                <div class="banner-content">
                    <span class="banner-icon">"⚠️"</span>
                    <span class="banner-text">"No internet connection. Trying to reconnect..."</span>
                </div>
            </div>
        </Show>
    }
}
