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
                <h1>"System Health"</h1>
                <div class="dashboard-controls">
                    <button class="button button-small" on:click=refresh>
                        "Refresh"
                    </button>
                </div>
            </div>
            
            <Suspense fallback=move || view! {
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading..."</p>
                </div>
            }>
                {move || {
                    health_resource.get().map(|result| {
                        match result {
                            Ok(data) => {
                                let timestamp = chrono::DateTime::from_timestamp(data.timestamp as i64, 0)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_else(|| "Unknown".to_string());

                                view! {
                                    <div class="health-dashboard">
                                        <div class="info-row">
                                            <span class="info-label">"Status:"</span>
                                            <span class="info-value">{data.status.clone()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">"API Version:"</span>
                                            <span class="info-value">{data.api_version.clone()}</span>
                                        </div>
                                        {data.cargo_package_version.as_ref().map(|v| view! {
                                            <div class="info-row">
                                                <span class="info-label">"Backend Version:"</span>
                                                <span class="info-value">{v.clone()}</span>
                                            </div>
                                        })}
                                        <div class="info-row">
                                            <span class="info-label">"Server Time:"</span>
                                            <span class="info-value">{timestamp}</span>
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            Err(e) => view! {
                                <div class="error-state">
                                    <h3>"Unable to fetch health"</h3>
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
    // Deprecated page: metrics not implemented. Keep a minimal message to avoid suggesting functionality.
    view! { <div style="padding: 16px;">"Metrics are not available."</div> }
}
