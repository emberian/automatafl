use crate::state::AppState;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn HomePage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let is_authenticated = move || app_state.is_authenticated();

    view! {
        <div class="home-page">
            <section class="hero">
                <h1 class="hero-title">"Welcome to Automatafl"</h1>
                <p class="hero-subtitle">
                    "A strategic particle movement game where you compete to guide the automaton to your goal"
                </p>
                
                <div class="hero-actions">
                    <Show
                        when=is_authenticated
                        fallback=|| view! {
                            <A href="/register" attr:class="button button-primary button-large">
                                "Get Started"
                            </A>
                            <A href="/login" attr:class="button button-secondary button-large">
                                "Login"
                            </A>
                        }
                    >
                        <A href="/games" attr:class="button button-primary button-large">
                            "View Games"
                        </A>
                        <A href="/games/create" attr:class="button button-secondary button-large">
                            "Create Game"
                        </A>
                        // Note: Matchmaking not yet implemented in backend
                        // <A href="/matchmaking" attr:class="button button-secondary button-large">
                        //     "Quick Match"
                        // </A>
                    </Show>
                </div>
            </section>
            
            <section class="features">
                <h2>"Game Features"</h2>
                <div class="feature-grid">
                    <div class="feature-card">
                        <div class="feature-icon">{"🎯"}</div>
                        <h3>"Strategic Gameplay"</h3>
                        <p>"Move particles strategically to control the automaton's path and reach your goal"</p>
                    </div>
                    
                    <div class="feature-card">
                        <div class="feature-icon">{"⚛️"}</div>
                        <h3>"Particle Physics"</h3>
                        <p>"Use attractors and repulsors to influence the automaton's movement"</p>
                    </div>
                    
                    <div class="feature-card">
                        <div class="feature-icon">{"🏆"}</div>
                        <h3>"Competitive Rating"</h3>
                        <p>"Climb the leaderboard and improve your rating through victories"</p>
                    </div>
                    
                    <div class="feature-card">
                        <div class="feature-icon">{"💬"}</div>
                        <h3>"Real-time Chat"</h3>
                        <p>"Communicate with your opponent during matches"</p>
                    </div>
                    
                    <div class="feature-card">
                        <div class="feature-icon">{"⚡"}</div>
                        <h3>"Quick Matchmaking"</h3>
                        <p>"Find opponents of similar skill level automatically"</p>
                    </div>
                    
                    <div class="feature-card">
                        <div class="feature-icon">{"👁️"}</div>
                        <h3>"Spectator Mode"</h3>
                        <p>"Watch live games and learn from other players"</p>
                    </div>
                </div>
            </section>
            
            <section class="how-to-play">
                <h2>"How to Play"</h2>
                <div class="instructions">
                    <div class="instruction-step">
                        <div class="step-number">"1"</div>
                        <div class="step-content">
                            <h4>"Understand the Goal"</h4>
                            <p>"Each player has a goal position on the board. Guide the automaton to your goal to win!"</p>
                        </div>
                    </div>
                    
                    <div class="instruction-step">
                        <div class="step-number">"2"</div>
                        <div class="step-content">
                            <h4>"Move Your Particles"</h4>
                            <p>"On your turn, move any particle (attractors or repulsors) to a new position"</p>
                        </div>
                    </div>
                    
                    <div class="instruction-step">
                        <div class="step-number">"3"</div>
                        <div class="step-content">
                            <h4>"Automaton Movement"</h4>
                            <p>"After both players move, the automaton moves based on the forces from all particles"</p>
                        </div>
                    </div>
                    
                    <div class="instruction-step">
                        <div class="step-number">"4"</div>
                        <div class="step-content">
                            <h4>"Strategy Tips"</h4>
                            <p>"Use attractors to pull the automaton towards your goal and repulsors to push it away from your opponent's"</p>
                        </div>
                    </div>
                </div>
            </section>
            
            <section class="particle-types">
                <h2>"Particle Types"</h2>
                <div class="particle-grid">
                    <div class="particle-card">
                        <div class="particle-visual attractor">{"⊕"}</div>
                        <h3>"Attractor"</h3>
                        <p>"Pulls the automaton towards itself with a force that decreases with distance"</p>
                    </div>
                    
                    <div class="particle-card">
                        <div class="particle-visual repulsor">{"⊖"}</div>
                        <h3>"Repulsor"</h3>
                        <p>"Pushes the automaton away with a force that decreases with distance"</p>
                    </div>
                    
                    <div class="particle-card">
                        <div class="particle-visual automaton">{"◉"}</div>
                        <h3>"Automaton"</h3>
                        <p>"The piece both players try to control. Moves based on net forces from all particles"</p>
                    </div>
                    
                    <div class="particle-card">
                        <div class="particle-visual vacuum">{"○"}</div>
                        <h3>"Vacuum"</h3>
                        <p>"Empty space where particles can be moved"</p>
                    </div>
                </div>
            </section>
        </div>
    }
}
