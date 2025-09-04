use crate::{api::ApiClient, state::AppState};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

#[component]
pub fn LoginPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let navigate = use_navigate();
    
    let (username, set_username) = create_signal(String::new());
    let (password, set_password) = create_signal(String::new());
    let (error, set_error) = create_signal(Option::<String>::None);
    let (loading, set_loading) = create_signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        
        let app_state = app_state.clone();
        let username = username.get();
        let password = password.get();
        
        if username.is_empty() || password.is_empty() {
            set_error(Some("Username and password are required".to_string()));
            return;
        }
        
        set_loading(true);
        set_error(None);
        
        spawn_local(async move {
            let client = ApiClient::new(app_state.api_base_url.clone(), None);
            
            match client.login(username, password).await {
                Ok(auth_response) => {
                    app_state.login(auth_response.token, auth_response.user);
                    navigate("/games", Default::default());
                }
                Err(e) => {
                    set_error(Some(e));
                    set_loading(false);
                }
            }
        });
    };

    view! {
        <div class="auth-container">
            <div class="auth-card">
                <h1>"Login to Automatafl"</h1>
                
                <form on:submit=on_submit class="auth-form">
                    <div class="form-group">
                        <label for="username">"Username"</label>
                        <input
                            type="text"
                            id="username"
                            class="form-input"
                            placeholder="Enter your username"
                            on:input=move |ev| set_username(event_target_value(&ev))
                            prop:value=username
                        />
                    </div>
                    
                    <div class="form-group">
                        <label for="password">"Password"</label>
                        <input
                            type="password"
                            id="password"
                            class="form-input"
                            placeholder="Enter your password"
                            on:input=move |ev| set_password(event_target_value(&ev))
                            prop:value=password
                        />
                    </div>
                    
                    <Show when=move || error.get().is_some()>
                        <div class="error-message">
                            {move || error.get().unwrap_or_default()}
                        </div>
                    </Show>
                    
                    <button
                        type="submit"
                        class="submit-button"
                        disabled=loading
                    >
                        {move || if loading.get() { "Logging in..." } else { "Login" }}
                    </button>
                </form>
                
                <div class="auth-footer">
                    <p>"Don't have an account? " <a href="/register">"Register here"</a></p>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn RegisterPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let navigate = use_navigate();
    
    let (username, set_username) = create_signal(String::new());
    let (password, set_password) = create_signal(String::new());
    let (confirm_password, set_confirm_password) = create_signal(String::new());
    let (error, set_error) = create_signal(Option::<String>::None);
    let (loading, set_loading) = create_signal(false);

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        
        let app_state = app_state.clone();
        let username = username.get();
        let password = password.get();
        let confirm_password = confirm_password.get();
        
        // Validation
        if username.is_empty() || password.is_empty() {
            set_error(Some("Username and password are required".to_string()));
            return;
        }
        
        if username.len() < 3 {
            set_error(Some("Username must be at least 3 characters".to_string()));
            return;
        }
        
        if password.len() < 6 {
            set_error(Some("Password must be at least 6 characters".to_string()));
            return;
        }
        
        if password != confirm_password {
            set_error(Some("Passwords do not match".to_string()));
            return;
        }
        
        set_loading(true);
        set_error(None);
        
        spawn_local(async move {
            let client = ApiClient::new(app_state.api_base_url.clone(), None);
            
            match client.register(username, password).await {
                Ok(auth_response) => {
                    app_state.login(auth_response.token, auth_response.user);
                    navigate("/games", Default::default());
                }
                Err(e) => {
                    set_error(Some(e));
                    set_loading(false);
                }
            }
        });
    };

    view! {
        <div class="auth-container">
            <div class="auth-card">
                <h1>"Create Your Account"</h1>
                <p class="auth-subtitle">"Join Automatafl and start playing strategic particle games!"</p>
                
                <form on:submit=on_submit class="auth-form">
                    <div class="form-group">
                        <label for="username">"Username"</label>
                        <input
                            type="text"
                            id="username"
                            class="form-input"
                            placeholder="Choose a username (min 3 characters)"
                            on:input=move |ev| set_username(event_target_value(&ev))
                            prop:value=username
                        />
                        <small class="form-hint">"This will be your display name in games"</small>
                    </div>
                    
                    <div class="form-group">
                        <label for="password">"Password"</label>
                        <input
                            type="password"
                            id="password"
                            class="form-input"
                            placeholder="Create a password (min 6 characters)"
                            on:input=move |ev| set_password(event_target_value(&ev))
                            prop:value=password
                        />
                    </div>
                    
                    <div class="form-group">
                        <label for="confirm-password">"Confirm Password"</label>
                        <input
                            type="password"
                            id="confirm-password"
                            class="form-input"
                            placeholder="Re-enter your password"
                            on:input=move |ev| set_confirm_password(event_target_value(&ev))
                            prop:value=confirm_password
                        />
                    </div>
                    
                    <Show when=move || error.get().is_some()>
                        <div class="error-message">
                            {move || error.get().unwrap_or_default()}
                        </div>
                    </Show>
                    
                    <div class="form-info">
                        <p>"By registering, you'll start with a rating of 1200"</p>
                    </div>
                    
                    <button
                        type="submit"
                        class="submit-button"
                        disabled=loading
                    >
                        {move || if loading.get() { "Creating Account..." } else { "Register" }}
                    </button>
                </form>
                
                <div class="auth-footer">
                    <p>"Already have an account? " <a href="/login">"Login here"</a></p>
                </div>
            </div>
        </div>
    }
}
