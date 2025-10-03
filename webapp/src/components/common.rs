// Common UI components used across the webapp

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
pub fn SkeletonList(
    #[prop(default = 3)] count: usize,
) -> impl IntoView {
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
pub fn StatusBadge(
    status: &'static str,
    variant: StatusVariant,
) -> impl IntoView {
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

/// Connection status indicator
#[component]
pub fn ConnectionStatus() -> impl IntoView {
    use crate::state::AppState;
    
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    view! {
        <div class="connection-status">
            {move || {
                let connected = app_state.websocket_connected.get();
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
        </div>
    }
}

/// Network status detector
#[component]
pub fn NetworkStatus() -> impl IntoView {
    let (is_online, set_is_online) = signal(true);
    
    Effect::new(move |_| {
        let window = web_sys::window().expect("window should exist");
        
        // Check initial online status
        // Note: navigator() method might need web-sys feature flag
        // For now, assume online
        set_is_online.set(true);
        
        // Listen for online/offline events
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;
        
        let online_callback = Closure::wrap(Box::new(move || {
            set_is_online.set(true);
            web_sys::console::log_1(&"📡 Network connection restored".into());
        }) as Box<dyn Fn()>);
        
        let offline_callback = Closure::wrap(Box::new(move || {
            set_is_online.set(false);
            web_sys::console::warn_1(&"📡 Network connection lost".into());
        }) as Box<dyn Fn()>);
        
        let _ = window.add_event_listener_with_callback(
            "online",
            online_callback.as_ref().unchecked_ref()
        );
        let _ = window.add_event_listener_with_callback(
            "offline",
            offline_callback.as_ref().unchecked_ref()
        );
        
        // Leak closures to keep them alive
        online_callback.forget();
        offline_callback.forget();
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

