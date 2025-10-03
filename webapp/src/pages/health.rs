use crate::{api::ApiClient, state::AppState};
use leptos::prelude::*;

#[component]
pub fn HealthDashboardPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let api_base_url = app_state.api_base_url.clone();
    let health_resource = LocalResource::new(
        move || {
            // Track refresh_trigger to make resource reactive
            let _ = refresh_trigger.get();
            let api_base_url = api_base_url.clone();
            async move {
                let client = ApiClient::new(api_base_url);
                client.health_check().await
            }
        }
    );
    
    let refresh = move |_| {
        set_refresh_trigger.update(|n| *n += 1);
    };

    view! {
        <div class="health-dashboard-page">
            <div class="dashboard-header">
                <h1>"🏥 System Health Dashboard"</h1>
                <div class="dashboard-controls">
                    <button class="button button-small" on:click=refresh>
                        "Refresh Now"
                    </button>
                </div>
            </div>
            
            <Suspense fallback=move || view! {
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Checking system health..."</p>
                </div>
            }>
                {move || {
                    health_resource.get().map(|result| {
                        match result {
                            Ok(data) => {
                                let status_class = if data.status == "healthy" {
                                    "status-healthy"
                                } else {
                                    "status-unhealthy"
                                };
                                
                                // Format timestamp
                                let timestamp = chrono::DateTime::from_timestamp(data.timestamp as i64, 0)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_else(|| "Unknown".to_string());
                                
                                view! {
                                    <div class="health-dashboard">
                                        <div class="health-overview">
                                            <div class=format!("overall-status {}", status_class)>
                                                <h2>"Overall Status"</h2>
                                                <div class="status-indicator">
                                                    {if data.status == "healthy" { "✅" } else { "❌" }}
                                                    <span>{data.status.to_uppercase()}</span>
                                                </div>
                                            </div>
                                            
                                            <div class="version-info">
                                                <h3>"API Version"</h3>
                                                <code>{data.api_version.clone()}</code>
                                            </div>
                                            
                                            {data.cargo_package_version.as_ref().map(|version| view! {
                                                <div class="version-info">
                                                    <h3>"Backend Version"</h3>
                                                    <code>{version.clone()}</code>
                                                </div>
                                            })}
                                        </div>
                                        
                                        <div class="health-sections">
                                            <div class="health-section">
                                                <h3>"🕐 Server Time"</h3>
                                                <div class="time-display">
                                                    <span class="time-value">{timestamp}</span>
                                                </div>
                                            </div>
                                            
                                            <div class="health-section">
                                                <h3>"🌐 API Endpoints"</h3>
                                                <div class="endpoints-info">
                                                    <p>"Automatafl backend is operational"</p>
                                                    <ul class="endpoint-list">
                                                        <li>"✓ Authentication endpoints"</li>
                                                        <li>"✓ Game management endpoints"</li>
                                                        <li>"✓ WebSocket connections"</li>
                                                        <li>"✓ Chat functionality"</li>
                                                        <li>"✓ Save/Load system"</li>
                                                    </ul>
                                                </div>
                                            </div>
                                        </div>
                                        
                                        <div class="last-updated">
                                            <small>"Last checked: " {chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()}</small>
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            Err(e) => view! {
                                <div class="error-state">
                                    <h3>"Unable to fetch health data"</h3>
                                    <p>{format!("{}", e)}</p>
                                    <button class="button" on:click=refresh>
                                        "Retry"
                                    </button>
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
pub fn MetricsPage() -> impl IntoView {
    // TODO: Metrics endpoint not yet implemented in backend
    view! {
        <div class="metrics-page">
            <div class="metrics-header">
                <h1>"📊 System Metrics"</h1>
            </div>
            
            <div class="stub-notice">
                <div class="stub-content">
                    <h2>"🚧 Coming Soon"</h2>
                    <p>"Detailed metrics endpoint is not yet implemented in the backend."</p>
                    <p>"Use the health dashboard for basic system status information."</p>
                    
                    <div class="stub-actions">
                        <a href="/health" class="button button-primary">
                            "View Health Dashboard"
                        </a>
                    </div>
                </div>
            </div>
        </div>
    }
}
