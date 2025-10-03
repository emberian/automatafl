// TODO: Matchmaking is not yet implemented in the backend
// This is a stub page that will be implemented when the backend supports it

use crate::state::AppState;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn MatchmakingPage() -> impl IntoView {
    let _app_state = use_context::<AppState>().expect("AppState should be provided");

    view! {
        <div class="matchmaking-page">
            <div class="page-header">
                <h1>"Quick Match"</h1>
            </div>
            
            <div class="stub-notice">
                <div class="stub-content">
                    <h2>"🚧 Coming Soon"</h2>
                    <p>"Matchmaking is not yet implemented in the backend."</p>
                    <p>"For now, you can create or join games manually from the games list."</p>
                    
                    <div class="stub-actions">
                        <A href="/games" attr:class="button button-primary">
                            "View Games"
                        </A>
                        <A href="/games/create" attr:class="button button-secondary">
                            "Create Game"
                        </A>
                    </div>
                </div>
            </div>
        </div>
    }
}
