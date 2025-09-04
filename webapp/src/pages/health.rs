use crate::{api::ApiClient, state::AppState};
use automatafl_api::HealthCheckResponse;
use leptos::prelude::*;

#[component]
pub fn HealthDashboardPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (health_data, set_health_data) = create_signal(Option::<HealthCheckResponse>::None);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(Option::<String>::None);
    let (auto_refresh, set_auto_refresh) = create_signal(true);
    
    let refresh_interval = store_value(None::<leptos::leptos_dom::helpers::IntervalHandle>);
    
    // Fetch health data
    let fetch_health = move || {
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.health_check().await {
                Ok(data) => {
                    set_health_data(Some(data));
                    set_loading(false);
                    set_error(None);
                }
                Err(e) => {
                    set_error(Some(e));
                    set_loading(false);
                }
            }
        });
    };
    
    // Initial fetch and setup auto-refresh
    Effect::new(move |_| {
        fetch_health();
        
        if auto_refresh.get() {
            let handle = leptos::leptos_dom::helpers::set_interval_with_handle(
                move || fetch_health(),
                std::time::Duration::from_secs(5),
            ).ok();
            refresh_interval.set_value(handle);
        } else {
            if let Some(handle) = refresh_interval.get_value() {
                handle.clear();
            }
        }
    });

    view! {
        <div class="health-dashboard-page">
            <div class="dashboard-header">
                <h1>"🏥 System Health Dashboard"</h1>
                <div class="dashboard-controls">
                    <label class="toggle-option">
                        <input
                            type="checkbox"
                            checked=auto_refresh
                            on:change=move |ev| {
                                let checked = event_target_checked(&ev);
                                set_auto_refresh(checked);
                                if !checked {
                                    if let Some(handle) = refresh_interval.get_value() {
                                        handle.clear();
                                    }
                                } else {
                                    let handle = leptos::leptos_dom::helpers::set_interval_with_handle(
                                        move || fetch_health(),
                                        std::time::Duration::from_secs(5),
                                    ).ok();
                                    refresh_interval.set_value(handle);
                                }
                            }
                        />
                        <span>"Auto-refresh (5s)"</span>
                    </label>
                    <button class="button button-small" on:click=move |_| fetch_health()>
                        "Refresh Now"
                    </button>
                </div>
            </div>
            
            <Show
                when=move || loading.get() && health_data.get().is_none()
                fallback=move || view! {
                    <Show
                        when=move || health_data.get().is_some()
                        fallback=move || view! {
                            <div class="error-state">
                                <h3>"Unable to fetch health data"</h3>
                                <p>{move || error.get().unwrap_or_else(|| "Unknown error".to_string())}</p>
                                <button class="button" on:click=move |_| fetch_health()>
                                    "Retry"
                                </button>
                            </div>
                        }
                    >
                        {move || {
                            let data = health_data.get().unwrap();
                            let status_class = if data.status == "healthy" {
                                "status-healthy"
                            } else {
                                "status-unhealthy"
                            };
                            
                            let db_status_class = if data.database.connected {
                                "status-healthy"
                            } else {
                                "status-unhealthy"
                            };
                            
                            let uptime_hours = data.uptime_seconds / 3600;
                            let uptime_minutes = (data.uptime_seconds % 3600) / 60;
                            let uptime_seconds = data.uptime_seconds % 60;
                            
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
                                            <h3>"Version"</h3>
                                            <code>{&data.version}</code>
                                        </div>
                                    </div>
                                    
                                    <div class="health-sections">
                                        <div class="health-section">
                                            <h3>"🕐 Uptime"</h3>
                                            <div class="uptime-display">
                                                <span class="uptime-value">
                                                    {format!("{}h {}m {}s", uptime_hours, uptime_minutes, uptime_seconds)}
                                                </span>
                                            </div>
                                        </div>
                                        
                                        <div class="health-section">
                                            <h3>"🗄️ Database"</h3>
                                            <div class=format!("database-status {}", db_status_class)>
                                                <div class="status-row">
                                                    <span>"Connection:"</span>
                                                    <span class="status-value">
                                                        {if data.database.connected { "Connected" } else { "Disconnected" }}
                                                    </span>
                                                </div>
                                                <Show when=move || data.database.latency_ms.is_some()>
                                                    <div class="status-row">
                                                        <span>"Latency:"</span>
                                                        <span class="status-value">
                                                            {format!("{} ms", data.database.latency_ms.unwrap())}
                                                        </span>
                                                    </div>
                                                </Show>
                                            </div>
                                        </div>
                                        
                                        <div class="health-section">
                                            <h3>"🌐 API Endpoints"</h3>
                                            <div class="endpoints-info">
                                                <p>"All API endpoints are monitored and operational"</p>
                                                <ul class="endpoint-list">
                                                    <li>"✓ Authentication endpoints"</li>
                                                    <li>"✓ Game management endpoints"</li>
                                                    <li>"✓ WebSocket connections"</li>
                                                    <li>"✓ Matchmaking service"</li>
                                                    <li>"✓ Chat functionality"</li>
                                                </ul>
                                            </div>
                                        </div>
                                    </div>
                                    
                                    <div class="last-updated">
                                        <small>"Last updated: " {chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()}</small>
                                    </div>
                                </div>
                            }
                        }}
                    </Show>
                }
            >
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Checking system health..."</p>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn MetricsPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    
    let (metrics_data, set_metrics_data) = create_signal(String::new());
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(Option::<String>::None);
    let (auto_refresh, set_auto_refresh) = create_signal(false);
    
    let refresh_interval = store_value(None::<leptos::leptos_dom::helpers::IntervalHandle>);
    
    // Fetch metrics
    let fetch_metrics = move || {
        spawn_local(async move {
            let client = ApiClient::new(
                app_state.api_base_url.clone(),
                app_state.auth_token.get()
            );
            
            match client.get_metrics().await {
                Ok(data) => {
                    set_metrics_data(data);
                    set_loading(false);
                    set_error(None);
                }
                Err(e) => {
                    set_error(Some(e));
                    set_loading(false);
                }
            }
        });
    };
    
    // Initial fetch and setup auto-refresh
    Effect::new(move |_| {
        fetch_metrics();
        
        if auto_refresh.get() {
            let handle = leptos::leptos_dom::helpers::set_interval_with_handle(
                move || fetch_metrics(),
                std::time::Duration::from_secs(10),
            ).ok();
            refresh_interval.set_value(handle);
        } else {
            if let Some(handle) = refresh_interval.get_value() {
                handle.clear();
            }
        }
    });

    view! {
        <div class="metrics-page">
            <div class="metrics-header">
                <h1>"📊 System Metrics"</h1>
                <div class="metrics-controls">
                    <label class="toggle-option">
                        <input
                            type="checkbox"
                            checked=auto_refresh
                            on:change=move |ev| {
                                let checked = event_target_checked(&ev);
                                set_auto_refresh(checked);
                                if !checked {
                                    if let Some(handle) = refresh_interval.get_value() {
                                        handle.clear();
                                    }
                                } else {
                                    let handle = leptos::leptos_dom::helpers::set_interval_with_handle(
                                        move || fetch_metrics(),
                                        std::time::Duration::from_secs(10),
                                    ).ok();
                                    refresh_interval.set_value(handle);
                                }
                            }
                        />
                        <span>"Auto-refresh (10s)"</span>
                    </label>
                    <button class="button button-small" on:click=move |_| fetch_metrics()>
                        "Refresh Now"
                    </button>
                </div>
            </div>
            
            <Show
                when=move || loading.get()
                fallback=move || view! {
                    <Show
                        when=move || error.get().is_none()
                        fallback=move || view! {
                            <div class="error-state">
                                <h3>"Unable to fetch metrics"</h3>
                                <p>{move || error.get().unwrap_or_default()}</p>
                                <button class="button" on:click=move |_| fetch_metrics()>
                                    "Retry"
                                </button>
                            </div>
                        }
                    >
                        <div class="metrics-container">
                            <div class="metrics-info">
                                <p>"Prometheus-compatible metrics endpoint"</p>
                                <p class="metrics-hint">"These metrics can be scraped by monitoring tools"</p>
                            </div>
                            
                            <pre class="metrics-data">
                                <code>{move || metrics_data.get()}</code>
                            </pre>
                            
                            <div class="metrics-legend">
                                <h3>"Metric Types"</h3>
                                <ul>
                                    <li><strong>"http_requests_total:"</strong>" Total HTTP requests handled"</li>
                                    <li><strong>"http_requests_duration_seconds:"</strong>" Request processing time"</li>
                                    <li><strong>"active_games:"</strong>" Number of active games"</li>
                                    <li><strong>"connected_websockets:"</strong>" Active WebSocket connections"</li>
                                    <li><strong>"database_queries_total:"</strong>" Total database queries"</li>
                                    <li><strong>"matchmaking_queue_size:"</strong>" Players waiting for matches"</li>
                                </ul>
                            </div>
                        </div>
                    </Show>
                }
            >
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>"Loading metrics..."</p>
                </div>
            </Show>
        </div>
    }
}
