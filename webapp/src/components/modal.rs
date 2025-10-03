// Modal/Dialog system for confirmations and custom dialogs
use leptos::prelude::*;
use leptos::prelude::AnyView;
use wasm_bindgen::JsCast;

// Global modal state
#[derive(Clone)]
pub struct ModalContext {
    is_open: RwSignal<bool>,
    content: RwSignal<Option<ModalContent>>,
}

#[derive(Clone)]
pub struct ModalContent {
    pub title: String,
    pub body: AnyView,
    pub on_confirm: Option<Callback<()>>,
    pub on_cancel: Option<Callback<()>>,
    pub confirm_text: String,
    pub cancel_text: String,
    pub variant: ModalVariant,
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
            content: RwSignal::new(None),
        }
    }

    pub fn show(
        &self,
        title: impl Into<String>,
        body: AnyView,
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
        body: AnyView,
        on_confirm: impl Fn() + 'static + Send + Sync,
        on_cancel: Option<impl Fn() + 'static + Send + Sync>,
        confirm_text: impl Into<String>,
        cancel_text: impl Into<String>,
        variant: ModalVariant,
    ) {
        let content = ModalContent {
            title: title.into(),
            body: body.clone(),
            on_confirm: Some(Callback::new(move |_| on_confirm())),
            on_cancel: on_cancel.map(|f| Callback::new(move |_| f())),
            confirm_text: confirm_text.into(),
            cancel_text: cancel_text.into(),
            variant,
        };

        self.content.set(Some(content));
        self.is_open.set(true);
    }

    pub fn confirm(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl Fn() + 'static + Send + Sync,
    ) {
        let body = view! { <p>{message.into()}</p> }.into_any();
        self.show(title, body, on_confirm, None::<fn()>);
    }

    pub fn confirm_danger(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
        on_confirm: impl Fn() + 'static + Send + Sync,
    ) {
        let body = view! { <p>{message.into()}</p> };
        self.show_with_options(
            title,
            body,
            on_confirm,
            None::<fn()>,
            "Delete",
            "Cancel",
            ModalVariant::Danger,
        );
    }

    pub fn alert(&self, title: impl Into<String>, message: impl Into<String>) {
        let body = view! { <p>{message.into()}</p> }.into_any();
        let content = ModalContent {
            title: title.into(),
            body: body,
            on_confirm: Some(Callback::new(move |_| {})),
            on_cancel: None,
            confirm_text: "OK".to_string(),
            cancel_text: "".to_string(),
            variant: ModalVariant::Default,
        };

        self.content.set(Some(content));
        self.is_open.set(true);
    }

    pub fn close(&self) {
        self.is_open.set(false);
        // Clear content after animation
        let content = self.content.clone();
        gloo_timers::callback::Timeout::new(300, move || {
            content.set(None);
        }).forget();
    }
}

/// Modal container component - add this to your app root
#[component]
pub fn ModalContainer() -> impl IntoView {
    let modal_ctx = use_context::<ModalContext>().expect("ModalContext should be provided");

    // Handle Escape key
    let modal_ctx_clone = modal_ctx.clone();
    Effect::new(move |_| {
        let modal_ctx = modal_ctx_clone.clone();
        if !modal_ctx.is_open.get() {
            return;
        }

        let window = web_sys::window().expect("window should exist");
        let document = window.document().expect("document should exist");

        use wasm_bindgen::prelude::*;

        let callback = Closure::wrap(Box::new(move |e: web_sys::KeyboardEvent| {
            if e.key() == "Escape" {
                modal_ctx.close();
            }
        }) as Box<dyn Fn(_)>);

        let _ = document.add_event_listener_with_callback(
            "keydown",
            callback.as_ref().unchecked_ref(),
        );

        // Cleanup
        callback.forget();
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

    let handle_backdrop_click = move |e: leptos::ev::MouseEvent| {
        // Only close if clicking directly on the backdrop, not its children
        if let Some(target) = e.target() {
            if let Some(element) = target.dyn_ref::<web_sys::HtmlElement>() {
                if element.class_list().contains("modal-backdrop") {
                    modal_ctx.close();
                }
            }
        }
    };

    let handle_confirm = move |_| {
        if let Some(content) = modal_ctx.content.get() {
            if let Some(on_confirm) = content.on_confirm {
                on_confirm.run(());
            }
        }
        modal_ctx.close();
    };

    let handle_cancel = move |_| {
        if let Some(content) = modal_ctx.content.get() {
            if let Some(on_cancel) = content.on_cancel {
                on_cancel.run(());
            }
        }
        modal_ctx.close();
    };

    view! {
        <div class="modal-backdrop" on:click=handle_backdrop_click>
            {move || {
                modal_ctx.content.get().map(|content| {
                    let variant_class = match content.variant {
                        ModalVariant::Default => "modal-default",
                        ModalVariant::Danger => "modal-danger",
                        ModalVariant::Warning => "modal-warning",
                        ModalVariant::Success => "modal-success",
                    };

                    let confirm_button_class = match content.variant {
                        ModalVariant::Danger => "button-danger",
                        ModalVariant::Warning => "button-warning",
                        ModalVariant::Success => "button-success",
                        _ => "button-primary",
                    };

                    view! {
                        <div class=format!("modal-dialog {}", variant_class)>
                            <div class="modal-header">
                                <h2 class="modal-title">{content.title.clone()}</h2>
                                <button
                                    class="modal-close"
                                    on:click=move |_| modal_ctx.close()
                                    aria-label="Close"
                                >
                                    "×"
                                </button>
                            </div>
                            <div class="modal-body">
                                {content.body.clone()}
                            </div>
                            <div class="modal-footer">
                                <Show when=move || content.on_cancel.is_some()>
                                    <button
                                        class="button button-secondary"
                                        on:click=handle_cancel
                                    >
                                        {content.cancel_text.clone()}
                                    </button>
                                </Show>
                                <button
                                    class=format!("button {}", confirm_button_class)
                                    on:click=handle_confirm
                                >
                                    {content.confirm_text.clone()}
                                </button>
                            </div>
                        </div>
                    }
                })
            }}
        </div>
    }
}

// Utility hook for easy access to modal context
pub fn use_modal() -> ModalContext {
    use_context::<ModalContext>().expect("ModalContext should be provided")
}

