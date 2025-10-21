// Toast notification system for user feedback
use leptos::prelude::*;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: usize,
    pub message: String,
    pub variant: ToastVariant,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Copy)]
pub enum ToastVariant {
    Success,
    Error,
    Warning,
    Info,
}

// Global toast state
#[derive(Clone)]
pub struct ToastContext {
    toasts: RwSignal<Vec<Toast>>,
    next_id: RwSignal<usize>,
}

impl ToastContext {
    pub fn new() -> Self {
        Self {
            toasts: RwSignal::new(Vec::new()),
            next_id: RwSignal::new(0),
        }
    }

    pub fn show(&self, message: impl Into<String>, variant: ToastVariant) {
        self.show_with_duration(message, variant, Duration::from_secs(4))
    }

    pub fn show_with_duration(
        &self,
        message: impl Into<String>,
        variant: ToastVariant,
        duration: Duration,
    ) {
        let id = self.next_id.get();
        self.next_id.update(|n| *n += 1);

        let toast = Toast {
            id,
            message: message.into(),
            variant,
            duration,
        };

        self.toasts.update(|toasts| toasts.push(toast));

        // Auto-dismiss after duration
        let toasts_clone = self.toasts;
        set_timeout(
            move || {
                toasts_clone.update(|toasts| {
                    toasts.retain(|t| t.id != id);
                });
            },
            duration,
        );
    }

    pub fn success(&self, message: impl Into<String>) {
        self.show(message, ToastVariant::Success);
    }

    pub fn error(&self, message: impl Into<String>) {
        self.show(message, ToastVariant::Error);
    }

    pub fn warning(&self, message: impl Into<String>) {
        self.show(message, ToastVariant::Warning);
    }

    pub fn info(&self, message: impl Into<String>) {
        self.show(message, ToastVariant::Info);
    }

    pub fn dismiss(&self, id: usize) {
        self.toasts.update(|toasts| {
            toasts.retain(|t| t.id != id);
        });
    }

    pub fn clear_all(&self) {
        self.toasts.update(|toasts| toasts.clear());
    }
}

/// Toast container component - add this to your app root
#[component]
pub fn ToastContainer() -> impl IntoView {
    let toast_ctx = use_context::<ToastContext>().expect("ToastContext should be provided");

    view! {
        <div class="toast-container">
            <For
                each=move || toast_ctx.toasts.get()
                key=|toast| toast.id
                children=move |toast| {
                    view! { <ToastItem toast=toast /> }
                }
            />
        </div>
    }
}

#[component]
fn ToastItem(toast: Toast) -> impl IntoView {
    let toast_ctx = use_context::<ToastContext>().expect("ToastContext should be provided");
    let id = toast.id;

    let (is_visible, set_is_visible) = signal(false);

    // Trigger entrance animation ONCE on mount
    Effect::new_isomorphic(move |prev_value: Option<()>| {
        // Only run on first execution (when prev_value is None)
        if prev_value.is_none() {
            gloo_timers::callback::Timeout::new(10, move || {
                set_is_visible.set(true);
            })
            .forget();
        }
        ()
    });

    let variant_class = match toast.variant {
        ToastVariant::Success => "toast-success",
        ToastVariant::Error => "toast-error",
        ToastVariant::Warning => "toast-warning",
        ToastVariant::Info => "toast-info",
    };

    let icon = match toast.variant {
        ToastVariant::Success => "✓",
        ToastVariant::Error => "✕",
        ToastVariant::Warning => "⚠",
        ToastVariant::Info => "ℹ",
    };

    view! {
        <div
            class=move || format!(
                "toast {} {}",
                variant_class,
                if is_visible.get() { "toast-visible" } else { "" }
            )
        >
            <div class="toast-icon">{icon}</div>
            <div class="toast-message">{toast.message.clone()}</div>
            <button
                class="toast-close"
                on:click=move |_| toast_ctx.dismiss(id)
                aria-label="Close"
            >
                "×"
            </button>
        </div>
    }
}

// Utility hook for easy access to toast context
pub fn use_toast() -> ToastContext {
    use_context::<ToastContext>().expect("ToastContext should be provided")
}
