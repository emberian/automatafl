// TODO: User profiles are not yet fully implemented in the backend
// These are stub pages that will be implemented when the backend supports user stats

use crate::state::AppState;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

#[component]
pub fn UserProfilePage() -> impl IntoView {
    let _app_state = use_context::<AppState>().expect("AppState should be provided");
    let params = use_params_map();
    let user_id = move || {
        params.get()
            .get("id")
            .and_then(|id| Uuid::parse_str(&id).ok())
    };

    view! {
        <div class="profile-page">
            <div class="page-header">
                <h1>"Player Profile"</h1>
                {move || user_id().map(|id| view! {
                    <p class="subtitle">"Player ID: " {id.to_string()}</p>
                })}
            </div>
            
            <div class="stub-notice">
                <div class="stub-content">
                    <h2>"🚧 Coming Soon"</h2>
                    <p>"Player profiles are not yet fully implemented in the backend."</p>
                    <p>"This feature will show player statistics, match history, and achievements."</p>
                    
                    <div class="stub-actions">
                        <A href="/games" attr:class="button button-primary">
                            "Back to Games"
                        </A>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn UserGamesPage() -> impl IntoView {
    let _app_state = use_context::<AppState>().expect("AppState should be provided");
    let params = use_params_map();
    let user_id = move || {
        params.get()
            .get("id")
            .and_then(|id| Uuid::parse_str(&id).ok())
    };

    view! {
        <div class="user-games-page">
            <div class="page-header">
                <h1>"Player Games"</h1>
                {move || user_id().map(|id| view! {
                    <p class="subtitle">"Player ID: " {id.to_string()}</p>
                })}
            </div>
            
            <div class="stub-notice">
                <div class="stub-content">
                    <h2>"🚧 Coming Soon"</h2>
                    <p>"Player game history is not yet implemented in the backend."</p>
                    <p>"This feature will show all past and ongoing games for a player."</p>
                    
                    <div class="stub-actions">
                        <A href="/games" attr:class="button button-primary">
                            "View All Games"
                        </A>
                    </div>
                </div>
            </div>
        </div>
    }
}
