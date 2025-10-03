// TODO: Leaderboard is not yet implemented in the backend
// This is a stub page that will be implemented when the backend supports ratings/rankings

use crate::state::AppState;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn LeaderboardPage() -> impl IntoView {
    let _app_state = use_context::<AppState>().expect("AppState should be provided");

    view! {
        <div class="leaderboard-page">
            <div class="page-header">
                <h1>"Leaderboard"</h1>
            </div>
            
            <div class="stub-notice">
                <div class="stub-content">
                    <h2>"🚧 Coming Soon"</h2>
                    <p>"The leaderboard and player ranking system are not yet implemented in the backend."</p>
                    <p>"This feature will track player ratings, wins, losses, and display global rankings."</p>
                    
                    <div class="stub-info">
                        <h3>"Planned Features:"</h3>
                        <ul>
                            <li>"ELO-based rating system"</li>
                            <li>"Win/loss records"</li>
                            <li>"Top players ranking"</li>
                            <li>"Player statistics and history"</li>
                        </ul>
                    </div>
                    
                    <div class="stub-actions">
                        <A href="/games" attr:class="button button-primary">
                            "Play Games"
                        </A>
                    </div>
                </div>
            </div>
        </div>
    }
}
