use crate::{api::ApiClient, state::AppState};
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

#[component]
pub fn UserProfilePage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let params = use_params_map();
    
    let user_id = Memo::new(move |_| {
        params.get()
            .get("id")
            .and_then(|id| Uuid::parse_str(&id).ok())
    });
    
    let api_base_url = app_state.api_base_url.clone();
    let profile_resource = LocalResource::new(move || {
        let uid = user_id.get();
        let api_base_url = api_base_url.clone();
        async move {
            match uid {
                Some(uid) => {
                    let client = ApiClient::new(api_base_url);
                    client.get_player_profile(uid).await
                }
                None => Err(automatafl_backend_client::ClientError::Api("Invalid user ID".to_string()))
            }
        }
    });
    
    let api_base_url_for_stats = app_state.api_base_url.clone();
    let stats_resource = LocalResource::new(move || {
        let uid = user_id.get();
        let api_base_url = api_base_url_for_stats.clone();
        async move {
            match uid {
                Some(uid) => {
                    let client = ApiClient::new(api_base_url);
                    client.get_player_stats(uid).await
                }
                None => Err(automatafl_backend_client::ClientError::Api("Invalid user ID".to_string()))
            }
        }
    });
    
    let is_own_profile = move || {
        user_id.get().and_then(|uid| {
            app_state.current_player_id.get().map(|pid| uid == pid)
        }).unwrap_or(false)
    };

    view! {
        <div class="profile-page">
            <Suspense fallback=move || view! {
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading profile..."</p>
                </div>
            }>
                {move || {
                    profile_resource.get().map(|profile_result| {
                        match profile_result {
                            Ok(profile) => view! {
                                <div class="profile-container">
                                    <div class="profile-header">
                                        <div class="profile-avatar">
                                            {if let Some(ref avatar_url) = profile.avatar_url {
                                                view! {
                                                    <img src=avatar_url.clone() alt="Avatar" />
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <div class="avatar-placeholder">
                                                        {profile.displayname.chars().next().unwrap_or('?').to_uppercase().to_string()}
                                                    </div>
                                                }.into_any()
                                            }}
                                        </div>
                                        
                                        <div class="profile-info">
                                            <h1>{profile.displayname.clone()}</h1>
                                            <div class="profile-elo">
                                                <span class="elo-label">"ELO: "</span>
                                                <span class="elo-value">{profile.elo_rating}</span>
                                            </div>
                                            {if let Some(ref bio) = profile.bio {
                                                view! {
                                                    <p class="profile-bio">{bio.clone()}</p>
                                                }.into_any()
                                            } else {
                                                view! {}.into_any()
                                            }}
                                            <p class="profile-joined">
                                                "Member since " {
                                                    chrono::DateTime::from_timestamp(profile.created_at as i64, 0)
                                                        .map(|dt| dt.format("%B %d, %Y").to_string())
                                                        .unwrap_or_else(|| "Unknown".to_string())
                                                }
                                            </p>
                                        </div>
                                    </div>
                                    
                                    {if is_own_profile() {
                                        let profile_id = profile.id;
                                        view! {
                                            <ProfileEditSection profile_id=profile_id initial_bio=profile.bio.clone() initial_avatar=profile.avatar_url.clone() />
                                        }.into_any()
                                    } else {
                                        view! {}.into_any()
                                    }}
                                    
                                    <Suspense fallback=move || view! {
                                        <div class="loading">Loading stats...</div>
                                    }>
                                        {move || {
                                            stats_resource.get().map(|stats_result| {
                                                match stats_result {
                                                    Ok(stats) => {
                                                        let games_lost = stats.games_played.saturating_sub(stats.games_won);
                                                        view! {
                                                            <div class="profile-stats">
                                                                <h2>"Statistics"</h2>
                                                                <div class="stats-grid">
                                                                    <div class="stat-card">
                                                                        <div class="stat-value">{stats.games_played}</div>
                                                                        <div class="stat-label">"Games Played"</div>
                                                                    </div>
                                                                    <div class="stat-card">
                                                                        <div class="stat-value">{stats.games_won}</div>
                                                                        <div class="stat-label">"Wins"</div>
                                                                    </div>
                                                                    <div class="stat-card">
                                                                        <div class="stat-value">{games_lost}</div>
                                                                        <div class="stat-label">"Losses"</div>
                                                                    </div>
                                                                    <div class="stat-card">
                                                                        <div class="stat-value">
                                                                            {format!("{:.1}%", stats.win_rate * 100.0)}
                                                                        </div>
                                                                        <div class="stat-label">"Win Rate"</div>
                                                                    </div>
                                                                    <div class="stat-card">
                                                                        <div class="stat-value">
                                                                            {format!("{}h", stats.total_playtime / 3600)}
                                                                        </div>
                                                                        <div class="stat-label">"Total Playtime"</div>
                                                                    </div>
                                                                </div>
                                                            </div>
                                                        }.into_any()
                                                    },
                                                    Err(e) => view! {
                                                        <div class="error-message">
                                                            "Failed to load stats: " {format!("{}", e)}
                                                        </div>
                                                    }.into_any()
                                                }
                                            })
                                        }}
                                    </Suspense>
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <div class="error-state">
                                    <h3>"Error loading profile"</h3>
                                    <p>{format!("{}", e)}</p>
                                    <A href="/leaderboard" attr:class="button">
                                        "View Leaderboard"
                                    </A>
                                </div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

#[component]
pub fn UserGamesPage() -> impl IntoView {
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

// ============================================================================
// Profile Edit Section
// ============================================================================

#[component]
fn ProfileEditSection(
    profile_id: Uuid,
    initial_bio: Option<String>,
    initial_avatar: Option<String>,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (is_editing, set_is_editing) = signal(false);
    let (bio, set_bio) = signal(initial_bio.clone().unwrap_or_default());
    let (avatar_url, set_avatar_url) = signal(initial_avatar.clone().unwrap_or_default());
    let (status, set_status) = signal(String::new());
    
    // Store initial values as signals so they're Copy
    let (initial_bio_signal, _) = signal(initial_bio.clone().unwrap_or_default());
    let (initial_avatar_signal, _) = signal(initial_avatar.clone().unwrap_or_default());
    
    let api_base_url = app_state.api_base_url.clone();
    let update_action = Action::new_local(move |(pid, b, a): &(Uuid, String, String)| {
        let pid = *pid;
        let bio = if b.is_empty() { None } else { Some(b.clone()) };
        let avatar = if a.is_empty() { None } else { Some(a.clone()) };
        let base_url = api_base_url.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.update_player_profile(pid, bio, avatar).await
        }
    });
    
    let _ = Effect::new(move |_| {
        if let Some(result) = update_action.value().get() {
            match result {
                Ok(_) => {
                    set_status.set("✅ Profile updated successfully!".to_string());
                    set_is_editing.set(false);
                    // Trigger page refresh after a delay
                    set_timeout(
                        move || {
                            if let Some(window) = web_sys::window() {
                                let _ = window.location().reload();
                            }
                        },
                        std::time::Duration::from_secs(1)
                    );
                }
                Err(e) => {
                    set_status.set(format!("❌ Failed to update profile: {}", e));
                }
            }
        }
    });
    
    view! {
        <div class="profile-edit-section">
            <Show
                when=move || is_editing.get()
                fallback=move || view! {
                    <div class="profile-actions">
                        <button
                            class="button button-primary"
                            on:click=move |_| set_is_editing.set(true)
                        >
                            "✏️ Edit Profile"
                        </button>
                    </div>
                }
            >
                <div class="edit-form">
                    <h3>"Edit Profile"</h3>
                    
                    <div class="form-group">
                        <label for="bio">"Bio"</label>
                        <textarea
                            id="bio"
                            class="form-input"
                            placeholder="Tell us about yourself..."
                            rows="4"
                            on:input=move |ev| set_bio.set(event_target_value(&ev))
                            prop:value=bio
                        />
                    </div>
                    
                    <div class="form-group">
                        <label for="avatar_url">"Avatar URL"</label>
                        <input
                            type="url"
                            id="avatar_url"
                            class="form-input"
                            placeholder="https://example.com/avatar.png"
                            on:input=move |ev| set_avatar_url.set(event_target_value(&ev))
                            prop:value=avatar_url
                        />
                        <small class="form-hint">"Link to your avatar image"</small>
                    </div>
                    
                    {move || {
                        let s = status.get();
                        if !s.is_empty() {
                            view! {
                                <div class="status-message">{s}</div>
                            }.into_any()
                        } else {
                            view! {}.into_any()
                        }
                    }}
                    
                    <div class="form-actions">
                        <button
                            class="button button-primary"
                            on:click=move |_| {
                                set_status.set("Saving...".to_string());
                                let _ = update_action.dispatch((profile_id, bio.get(), avatar_url.get()));
                            }
                            disabled=move || update_action.pending().get()
                        >
                            {move || if update_action.pending().get() { "Saving..." } else { "Save Changes" }}
                        </button>
                        <button
                            class="button button-secondary"
                            on:click=move |_| {
                                set_bio.set(initial_bio_signal.get());
                                set_avatar_url.set(initial_avatar_signal.get());
                                set_is_editing.set(false);
                                set_status.set(String::new());
                            }
                            disabled=move || update_action.pending().get()
                        >
                            "Cancel"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}
