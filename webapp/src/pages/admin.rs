use crate::{components::AdminPanel, state::AppState};
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn AdminPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");

    // Check if user has admin privileges
    let is_admin = Signal::derive(move || app_state.is_admin());

    view! {
        <div class="admin-page">
            <Show
                when=move || is_admin.get()
                fallback=|| view! {
                    <div class="auth-required">
                        <h1>"Admin Access Required"</h1>
                        <p>"You must be logged in with administrator privileges to access this page."</p>
                        <A href="/login" attr:class="button button-primary">
                            "Login"
                        </A>
                    </div>
                }
            >
                <AdminPanel />
            </Show>
        </div>
    }
}
