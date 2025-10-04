// Modal/Dialog system for confirmations and custom dialogs
use leptos::prelude::*;
use leptos::prelude::window_event_listener;
use wasm_bindgen::JsCast;

// Global modal state
#[derive(Clone)]
pub struct ModalContext {
    is_open: RwSignal<bool>,
    title: RwSignal<String>,
    body: RwSignal<String>,
    on_confirm: RwSignal<Option<Callback<()>>>,
    on_cancel: RwSignal<Option<Callback<()>>>,
    confirm_text: RwSignal<String>,
    cancel_text: RwSignal<String>,
    variant: RwSignal<ModalVariant>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ModalVariant {
    Default,
    Danger,
    Warning,
    Success,
}

impl ModalContext {
    pub fn new() -> Self {
        Self {
            is_open: RwSignal::new(false),
            title: RwSignal::new(String::new()),
            body: RwSignal::new(String::new()),
            on_confirm: RwSignal::new(None),
            on_cancel: RwSignal::new(None),
            confirm_text: RwSignal::new("Confirm".to_string()),
            cancel_text: RwSignal::new("Cancel".to_string()),
            variant: RwSignal::new(ModalVariant::Default),
        }
    }

    pub fn show(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
        on_confirm: impl Fn() + 'static + Send + Sync,
        on_cancel: Option<impl Fn() + 'static + Send + Sync>,
    ) {
        self.show_with_options(
            title,
            body,
            on_confirm,
            on_cancel,
            "Confirm",
            "Cancel",
            ModalVariant::Default,
        );
    }

    pub fn show_with_options(
        &self,
        title: impl Into<String>,
        body: impl Into<String>,
        on_confirm: impl Fn() + 'static + Send + Sync,
        on_cancel: Option<impl Fn() + 'static + Send + Sync>,
        confirm_text: impl Into<String>,
        cancel_text: impl Into<String>,
        variant: ModalVariant,
    ) {
        self.title.set(title.into());
        self.body.set(body.into());
        self.on_confirm
            .set(Some(Callback::new(move |_| on_confirm())));
        self.on_cancel
            .set(on_cancel.map(|f| Callback::new(move |_| f())));
        self.confirm_text.set(confirm_text.into());
        self.cancel_text.set(cancel_text.into());
        self.variant.set(variant);
        self.is_open.set(true);
    }

    pub fn confirm(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl Fn() + 'static + Send + Sync,
    ) {
        self.show(title, message, on_confirm, None::<fn()>);
    }

    pub fn confirm_danger(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl Fn() + 'static + Send + Sync,
    ) {
        self.show_with_options(
            title,
            message,
            on_confirm,
            None::<fn()>,
            "Delete",
            "Cancel",
            ModalVariant::Danger,
        );
    }

    pub fn alert(&self, title: impl Into<String>, message: impl Into<String>) {
        self.title.set(title.into());
        self.body.set(message.into());
        self.on_confirm.set(Some(Callback::new(move |_| {})));
        self.on_cancel.set(None);
        self.confirm_text.set("OK".to_string());
        self.cancel_text.set("".to_string());
        self.variant.set(ModalVariant::Default);
        self.is_open.set(true);
    }

    pub fn close(&self) {
        self.is_open.set(false);
        // Clear content after animation
        let title = self.title.clone();
        let body = self.body.clone();
        let on_confirm = self.on_confirm.clone();
        let on_cancel = self.on_cancel.clone();
        gloo_timers::callback::Timeout::new(300, move || {
            title.set(String::new());
            body.set(String::new());
            on_confirm.set(None);
            on_cancel.set(None);
        })
        .forget();
    }
}

/// Modal container component - add this to your app root
#[component]
pub fn ModalContainer() -> impl IntoView {
    let modal_ctx = use_context::<ModalContext>().expect("ModalContext should be provided");

    // Use window_event_listener for proper automatic cleanup
    // This is much simpler and more reliable than manual Closure management
    let modal_ctx_for_listener = modal_ctx.clone();
    window_event_listener(leptos::ev::keydown, move |e: web_sys::KeyboardEvent| {
        if modal_ctx_for_listener.is_open.get() && e.key() == "Escape" {
            modal_ctx_for_listener.close();
        }
    });

    view! {
        <Show when=move || modal_ctx.is_open.get()>
            <ModalDialog />
        </Show>
    }
}

#[component]
fn ModalDialog() -> impl IntoView {
    let modal_ctx = use_context::<ModalContext>().expect("ModalContext should be provided");

    let modal_ctx_for_backdrop = modal_ctx.clone();
    let modal_ctx_for_class = modal_ctx.clone();
    let modal_ctx_for_title = modal_ctx.clone();
    let modal_ctx_for_body = modal_ctx.clone();
    let modal_ctx_for_button_class = modal_ctx.clone();
    let modal_ctx_for_confirm_text = modal_ctx.clone();
    let modal_ctx_for_has_cancel = modal_ctx.clone();
    let modal_ctx_close = modal_ctx.clone();
    let modal_ctx_confirm = modal_ctx.clone();
    let modal_ctx_cancel = modal_ctx.clone();

    let handle_backdrop_click = move |e: leptos::ev::MouseEvent| {
        // Only close if clicking directly on the backdrop, not its children
        if let Some(target) = e.target() {
            if let Some(element) = target.dyn_ref::<web_sys::HtmlElement>() {
                if element.class_list().contains("modal-backdrop") {
                    modal_ctx_for_backdrop.close();
                }
            }
        }
    };

    let handle_confirm = move |_: leptos::ev::MouseEvent| {
        if let Some(cb) = modal_ctx_confirm.on_confirm.get() {
            cb.run(());
        }
        modal_ctx_confirm.close();
    };

    let has_cancel = Signal::derive(move || modal_ctx_for_has_cancel.on_cancel.get().is_some());

    view! {
        <div class="modal-backdrop" on:click=handle_backdrop_click>
            <div class=move || {
                let variant = modal_ctx_for_class.variant.get();
                let variant_class = match variant {
                    ModalVariant::Default => "modal-default",
                    ModalVariant::Danger => "modal-danger",
                    ModalVariant::Warning => "modal-warning",
                    ModalVariant::Success => "modal-success",
                };
                format!("modal-dialog {}", variant_class)
            }>
                <div class="modal-header">
                    <h2 class="modal-title">{move || modal_ctx_for_title.title.get()}</h2>
                    <button
                        class="modal-close"
                        on:click=move |_| modal_ctx_close.close()
                        aria-label="Close"
                    >
                        "×"
                    </button>
                </div>
                <div class="modal-body">
                    <p>{move || modal_ctx_for_body.body.get()}</p>
                </div>
                <div class="modal-footer">
                    {move || {
                        let modal_ctx_cancel_btn = modal_ctx_cancel.clone();
                        has_cancel.get().then(|| view! {
                            <button
                                class="button button-secondary"
                                on:click=move |_| {
                                    if let Some(cb) = modal_ctx_cancel_btn.on_cancel.get() {
                                        cb.run(());
                                    }
                                    modal_ctx_cancel_btn.close();
                                }
                            >
                                {move || modal_ctx_cancel_btn.cancel_text.get()}
                            </button>
                        })
                    }}
                    <button
                        class=move || {
                            let variant = modal_ctx_for_button_class.variant.get();
                            let confirm_button_class = match variant {
                                ModalVariant::Danger => "button-danger",
                                ModalVariant::Warning => "button-warning",
                                ModalVariant::Success => "button-success",
                                _ => "button-primary",
                            };
                            format!("button {}", confirm_button_class)
                        }
                        on:click=handle_confirm
                    >
                        {move || modal_ctx_for_confirm_text.confirm_text.get()}
                    </button>
                </div>
            </div>
        </div>
    }
}

// Utility hook for easy access to modal context
pub fn use_modal() -> ModalContext {
    use_context::<ModalContext>().expect("ModalContext should be provided")
}
