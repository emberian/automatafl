use crate::api::ApiClient;
use automatafl_api::{AuthResponse, UserInfo};
use leptos::prelude::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthState {
    pub user: ReadSignal<Option<UserInfo>>,
    pub set_user: WriteSignal<Option<UserInfo>>,
    pub token: Arc<RwSignal<Option<String>>>,
}

impl AuthState {
    pub fn new() -> Self {
        let (user, set_user) = create_signal(None);
        let token = Arc::new(RwSignal::new(None));
        
        // Try to load token from localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                if let Ok(Some(stored_token)) = storage.get_item("auth_token") {
                    token.set(Some(stored_token));
                }
            }
        }
        
        Self { user, set_user, token }
    }

    pub fn is_authenticated(&self) -> bool {
        self.user.get().is_some()
    }

    pub fn logout(&self) {
        self.set_user.set(None);
        self.token.set(None);
        
        // Clear localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.remove_item("auth_token");
            }
        }
    }

    pub fn set_auth(&self, auth: AuthResponse) {
        self.set_user.set(Some(auth.user));
        self.token.set(Some(auth.token.clone()));
        
        // Store in localStorage
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("auth_token", &auth.token);
            }
        }
    }
}

#[component]
pub fn LoginForm(auth: AuthState, api: ApiClient) -> impl IntoView {
    let (username, set_username) = create_signal(String::new());
    let (password, set_password) = create_signal(String::new());
    let (error, set_error) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let auth = auth.clone();
        let api = api.clone();
        
        spawn_local(async move {
            set_loading.set(true);
            set_error.set(None);
            
            match api.login(username.get(), password.get()).await {
                Ok(auth_response) => {
                    auth.set_auth(auth_response);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            
            set_loading.set(false);
        });
    };

    view! {
        <div class="auth-form">
            <h2>"Login"</h2>
            <form on:submit=on_submit>
                <div class="form-group">
                    <label for="username">"Username"</label>
                    <input
                        type="text"
                        id="username"
                        class="form-control"
                        placeholder="Enter username"
                        on:input=move |ev| set_username.set(event_target_value(&ev))
                        prop:value=username
                        required
                    />
                </div>
                <div class="form-group">
                    <label for="password">"Password"</label>
                    <input
                        type="password"
                        id="password"
                        class="form-control"
                        placeholder="Enter password"
                        on:input=move |ev| set_password.set(event_target_value(&ev))
                        prop:value=password
                        required
                    />
                </div>
                {move || error.get().map(|e| view! {
                    <div class="alert alert-error">{e}</div>
                })}
                <button
                    type="submit"
                    class="btn btn-primary"
                    disabled=loading
                >
                    {move || if loading.get() { "Logging in..." } else { "Login" }}
                </button>
            </form>
        </div>
    }
}

#[component]
pub fn RegisterForm(auth: AuthState, api: ApiClient) -> impl IntoView {
    let (username, set_username) = create_signal(String::new());
    let (password, set_password) = create_signal(String::new());
    let (confirm_password, set_confirm_password) = create_signal(String::new());
    let (error, set_error) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        
        if password.get() != confirm_password.get() {
            set_error.set(Some("Passwords do not match".to_string()));
            return;
        }
        
        let auth = auth.clone();
        let api = api.clone();
        
        spawn_local(async move {
            set_loading.set(true);
            set_error.set(None);
            
            match api.register(username.get(), password.get()).await {
                Ok(auth_response) => {
                    auth.set_auth(auth_response);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            
            set_loading.set(false);
        });
    };

    view! {
        <div class="auth-form">
            <h2>"Register"</h2>
            <form on:submit=on_submit>
                <div class="form-group">
                    <label for="reg-username">"Username"</label>
                    <input
                        type="text"
                        id="reg-username"
                        class="form-control"
                        placeholder="Choose a username"
                        on:input=move |ev| set_username.set(event_target_value(&ev))
                        prop:value=username
                        required
                    />
                </div>
                <div class="form-group">
                    <label for="reg-password">"Password"</label>
                    <input
                        type="password"
                        id="reg-password"
                        class="form-control"
                        placeholder="Choose a password"
                        on:input=move |ev| set_password.set(event_target_value(&ev))
                        prop:value=password
                        required
                    />
                </div>
                <div class="form-group">
                    <label for="reg-confirm-password">"Confirm Password"</label>
                    <input
                        type="password"
                        id="reg-confirm-password"
                        class="form-control"
                        placeholder="Confirm your password"
                        on:input=move |ev| set_confirm_password.set(event_target_value(&ev))
                        prop:value=confirm_password
                        required
                    />
                </div>
                {move || error.get().map(|e| view! {
                    <div class="alert alert-error">{e}</div>
                })}
                <button
                    type="submit"
                    class="btn btn-primary"
                    disabled=loading
                >
                    {move || if loading.get() { "Registering..." } else { "Register" }}
                </button>
            </form>
        </div>
    }
}

#[component]
pub fn AuthSection(auth: AuthState, api: ApiClient) -> impl IntoView {
    let (show_register, set_show_register) = create_signal(false);

    view! {
        <div class="auth-section">
            {move || if auth.is_authenticated() {
                if let Some(user) = auth.user.get() {
                    view! {
                        <div class="user-info">
                            <h3>"Welcome, " {&user.username} "!"</h3>
                            <p>"Rating: " {user.rating}</p>
                            <button
                                class="btn btn-secondary"
                                on:click=move |_| auth.logout()
                            >
                                "Logout"
                            </button>
                        </div>
                    }.into_view()
                } else {
                    view! { <div></div> }.into_view()
                }
            } else {
                view! {
                    <div class="auth-container">
                        <div class="auth-toggle">
                            <button
                                class={move || if !show_register.get() { "active" } else { "" }}
                                on:click=move |_| set_show_register.set(false)
                            >
                                "Login"
                            </button>
                            <button
                                class={move || if show_register.get() { "active" } else { "" }}
                                on:click=move |_| set_show_register.set(true)
                            >
                                "Register"
                            </button>
                        </div>
                        {move || if show_register.get() {
                            view! { <RegisterForm auth=auth.clone() api=api.clone() /> }.into_view()
                        } else {
                            view! { <LoginForm auth=auth.clone() api=api.clone() /> }.into_view()
                        }}
                    </div>
                }.into_view()
            }}
        </div>
    }
}
