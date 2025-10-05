use crate::{components::use_toast, state::AppState};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

#[component]
pub fn LoginPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let navigate = use_navigate();
    let toast = use_toast();

    // Redirect if already logged in - only track is_authenticated signal
    let navigate_for_redirect = navigate.clone();
    Effect::new(move |_| {
        if app_state.is_authenticated.get() {
            navigate_for_redirect("/", Default::default());
        }
    });

    let (displayname, set_displayname) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);

    let app_state_for_login_action = app_state.clone();
    let login_action = Action::new_local(move |(dn, pw): &(String, String)| {
        let app_state = app_state_for_login_action.clone();
        let dn = dn.clone();
        let pw = pw.clone();
        async move {
            let mut client = app_state.get_api_client();
            client.login(dn, pw).await
        }
    });

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        let displayname_val = displayname.get_untracked();
        let password_val = password.get_untracked();

        if displayname_val.is_empty() || password_val.is_empty() {
            set_error.set(Some("Display name and password are required".to_string()));
            return;
        }

        set_error.set(None);
        login_action.dispatch((displayname_val, password_val));
    };

    Effect::new(move |_| {
        // CRITICAL: Call .with() to subscribe to value changes in a single reactive read
        // Previously this was creating TWO subscriptions (value() + get()) causing double-renders
        login_action.value().with(|result| {
            match result {
                Some(Ok(login_response)) => {
                    app_state.login(
                        login_response.session_id,
                        login_response.player_id,
                        login_response.is_admin,
                    );
                    toast.success("Login successful! Redirecting...");
                    navigate("/", Default::default());
                }
                Some(Err(e)) => {
                    toast.error(format!("Login failed: {}", e));
                    set_error.set(None); // Clear inline error if present
                }
                None => {}
            }
        });
    });

    view! {
        <div class="auth-container">
            <div class="auth-card">
                <h1>"Login to Automatafl"</h1>

                <form on:submit=on_submit class="auth-form">
                    <div class="form-group">
                        <label for="displayname">"Display Name"</label>
                        <input
                            type="text"
                            id="displayname"
                            class="form-input"
                            placeholder="Enter your display name"
                            prop:value=displayname
                            on:input=move |ev| set_displayname.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="form-group">
                        <label for="password">"Password"</label>
                        <input
                            type="password"
                            id="password"
                            class="form-input"
                            placeholder="Enter your password"
                            prop:value=password
                            on:input=move |ev| set_password.set(event_target_value(&ev))
                        />
                    </div>

                    {move || error.get().map(|e| view! {
                        <div class="error-message">{e}</div>
                    })}

                    <button
                        type="submit"
                        class="submit-button"
                        disabled=login_action.pending()
                    >
                        {move || if login_action.pending().get() { "Logging in..." } else { "Login" }}
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
    let toast = use_toast();

    // Redirect if already logged in - only track is_authenticated signal
    let navigate_for_redirect = navigate.clone();
    Effect::new(move |_| {
        if app_state.is_authenticated.get() {
            navigate_for_redirect("/", Default::default());
        }
    });

    let (displayname, set_displayname) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (confirm_password, set_confirm_password) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);

    let register_action = Action::new_local(move |(dn, pw): &(String, String)| {
        let app_state = app_state.clone();
        let dn = dn.clone();
        let pw = pw.clone();
        async move {
            let client = app_state.get_api_client();
            client.register(dn, pw).await
        }
    });

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        let displayname_val = displayname.get_untracked();
        let password_val = password.get_untracked();
        let confirm_password_val = confirm_password.get_untracked();

        // Validation
        if displayname_val.is_empty() || password_val.is_empty() {
            set_error.set(Some("Display name and password are required".to_string()));
            return;
        }

        if displayname_val.len() < 3 {
            set_error.set(Some(
                "Display name must be at least 3 characters".to_string(),
            ));
            return;
        }

        if password_val.len() < 6 {
            set_error.set(Some("Password must be at least 6 characters".to_string()));
            return;
        }

        if password_val != confirm_password_val {
            set_error.set(Some("Passwords do not match".to_string()));
            return;
        }

        set_error.set(None);
        register_action.dispatch((displayname_val, password_val));
    };

    let navigate_clone = navigate.clone();
    Effect::new(move |_| {
        // CRITICAL: Call .with() to subscribe to value changes in a single reactive read
        // Previously this was creating TWO subscriptions (value() + get()) causing double-renders
        register_action.value().with(|result| {
            match result {
                Some(Ok(_register_response)) => {
                    toast.success("Account created successfully! Redirecting to login...");
                    set_error.set(None);

                    // Redirect to login after 2 seconds
                    let nav = navigate_clone.clone();
                    set_timeout(
                        move || {
                            nav("/login", Default::default());
                        },
                        std::time::Duration::from_secs(2),
                    );
                }
                Some(Err(e)) => {
                    toast.error(format!("Registration failed: {}", e));
                    set_error.set(None);
                }
                None => {}
            }
        });
    });

    view! {
        <div class="auth-container">
            <div class="auth-card">
                <h1>"Create Your Account"</h1>
                <p class="auth-subtitle">"Join Automatafl and start playing!"</p>

                <form on:submit=on_submit class="auth-form">
                    <div class="form-group">
                        <label for="displayname">"Display Name"</label>
                        <input
                            type="text"
                            id="displayname"
                            class="form-input"
                            placeholder="Choose a display name (min 3 characters)"
                            prop:value=displayname
                            on:input=move |ev| set_displayname.set(event_target_value(&ev))
                        />
                        <small class="form-hint">"This will be your name in games"</small>
                    </div>

                    <div class="form-group">
                        <label for="password">"Password"</label>
                        <input
                            type="password"
                            id="password"
                            class="form-input"
                            placeholder="Create a password (min 6 characters)"
                            prop:value=password
                            on:input=move |ev| set_password.set(event_target_value(&ev))
                        />
                    </div>

                    <div class="form-group">
                        <label for="confirm-password">"Confirm Password"</label>
                        <input
                            type="password"
                            id="confirm-password"
                            class="form-input"
                            placeholder="Re-enter your password"
                            prop:value=confirm_password
                            on:input=move |ev| set_confirm_password.set(event_target_value(&ev))
                        />
                    </div>

                    {move || error.get().map(|e| view! {
                        <div class="error-message">{e}</div>
                    })}

                    <button
                        type="submit"
                        class="submit-button"
                        disabled=register_action.pending()
                    >
                        {move || if register_action.pending().get() { "Creating Account..." } else { "Register" }}
                    </button>
                </form>

                <div class="auth-footer">
                    <p>"Already have an account? " <a href="/login">"Login here"</a></p>
                </div>
            </div>
        </div>
    }
}
