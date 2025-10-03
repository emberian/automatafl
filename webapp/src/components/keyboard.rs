// Keyboard shortcuts system
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use std::collections::HashMap;

#[derive(Clone)]
pub struct KeyboardContext {
    shortcuts: RwSignal<HashMap<String, Callback<()>>>,
    help_visible: RwSignal<bool>,
}

impl KeyboardContext {
    pub fn new() -> Self {
        let ctx = Self {
            shortcuts: RwSignal::new(HashMap::new()),
            help_visible: RwSignal::new(false),
        };

        // Setup global keyboard listener
        ctx.setup_listener();
        ctx
    }

    fn setup_listener(&self) {
        let window = web_sys::window().expect("window should exist");
        let document = window.document().expect("document should exist");

        let shortcuts_clone = self.shortcuts;
        let help_visible = self.help_visible;

        let callback = Closure::wrap(Box::new(move |e: web_sys::KeyboardEvent| {
            // Don't capture keystrokes in input fields
            if let Some(target) = e.target() {
                if let Some(element) = target.dyn_ref::<web_sys::HtmlElement>() {
                    let tag_name = element.tag_name().to_lowercase();
                    if tag_name == "input" || tag_name == "textarea" {
                        return;
                    }
                }
            }

            // Build the keyboard shortcut string
            let mut parts = Vec::new();
            if e.ctrl_key() || e.meta_key() {
                parts.push("Ctrl");
            }
            if e.shift_key() {
                parts.push("Shift");
            }
            if e.alt_key() {
                parts.push("Alt");
            }
            let key = e.key();
            parts.push(&key);

            let shortcut_str = parts.join("+");

            // Handle "?" for help
            if e.key() == "?" && !e.ctrl_key() && !e.meta_key() {
                help_visible.update(|v| *v = !*v);
                e.prevent_default();
                return;
            }

            // Check if we have a handler for this shortcut
            if let Some(handler) = shortcuts_clone.get().get(&shortcut_str) {
                handler.run(());
                e.prevent_default();
            }
        }) as Box<dyn Fn(_)>);

        let _ = document.add_event_listener_with_callback(
            "keydown",
            callback.as_ref().unchecked_ref(),
        );

        callback.forget();
    }

    pub fn register(&self, shortcut: impl Into<String>, handler: impl Fn() + 'static + Send + Sync) {
        let shortcut_str = shortcut.into();
        self.shortcuts.update(|shortcuts| {
            shortcuts.insert(shortcut_str, Callback::new(move |_| handler()));
        });
    }

    pub fn unregister(&self, shortcut: impl Into<String>) {
        let shortcut_str = shortcut.into();
        self.shortcuts.update(|shortcuts| {
            shortcuts.remove(&shortcut_str);
        });
    }

    pub fn show_help(&self) {
        self.help_visible.set(true);
    }

    pub fn hide_help(&self) {
        self.help_visible.set(false);
    }
}

/// Keyboard shortcuts help overlay
#[component]
pub fn KeyboardShortcutsHelp() -> impl IntoView {
    let kb_ctx = use_context::<KeyboardContext>().expect("KeyboardContext should be provided");

    // Default shortcuts documentation
    let shortcuts = vec![
        ("?", "Show/Hide Shortcuts Help"),
        ("Escape", "Close Modal/Dialog"),
        ("Ctrl+K", "Quick Actions (coming soon)"),
    ];

    view! {
        <Show when=move || {
            let kb_ctx = kb_ctx.clone();
            kb_ctx.help_visible.get()
        }>
            <div
                class="keyboard-help-backdrop"
                on:click=move |_| {
                    let kb_ctx = kb_ctx.clone();
                    kb_ctx.hide_help()
                }
            >
                <div
                    class="keyboard-help-dialog"
                    on:click=move |e| e.stop_propagation()
                >
                    <div class="keyboard-help-header">
                        <h2>"⌨️ Keyboard Shortcuts"</h2>
                        <button
                            class="modal-close"
                            on:click=move |_| {
                                let kb_ctx = kb_ctx.clone();
                                kb_ctx.hide_help()
                            }
                        >
                            "×"
                        </button>
                    </div>
                    <div class="keyboard-help-body">
                        <div class="shortcuts-list">
                            {shortcuts.into_iter().map(|(key, description)| {
                                view! {
                                    <div class="shortcut-item">
                                        <kbd class="shortcut-key">{key}</kbd>
                                        <span class="shortcut-description">{description}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                        <div class="keyboard-help-footer">
                            <p class="help-hint">"Press ? again to close this dialog"</p>
                        </div>
                    </div>
                </div>
            </div>
        </Show>
    }
}

/// Hook to register a keyboard shortcut (auto-cleanup on unmount)
pub fn use_keyboard_shortcut(shortcut: impl Into<String>, handler: impl Fn() + 'static + Clone + Send + Sync) {
    let kb_ctx = use_context::<KeyboardContext>().expect("KeyboardContext should be provided");
    let shortcut_str = shortcut.into();
    let shortcut_clone = shortcut_str.clone();

    Effect::new(move |_| {
        let kb_ctx = kb_ctx.clone();
        let shortcut_str = shortcut_str.clone();
        let handler = handler.clone();
        let kb_ctx_cleanup = kb_ctx.clone();
        let shortcut_clone = shortcut_clone.clone();

        kb_ctx.register(shortcut_str, handler);

        // Cleanup on unmount
        move || {
            kb_ctx_cleanup.unregister(shortcut_clone);
        }
    });
}

pub fn use_keyboard() -> KeyboardContext {
    use_context::<KeyboardContext>().expect("KeyboardContext should be provided")
}

