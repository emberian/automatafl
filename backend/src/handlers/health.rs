use axum::{extract::State, Json};
use std::time::Instant;

use automatafl_api::{HealthCheckResponse, DatabaseHealth};

use crate::{
    error::Result,
    state::AppState,
};

// Store the server start time
static SERVER_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn init_health_check() {
    SERVER_START.set(Instant::now()).ok();
}

pub async fn health_check(State(state): State<AppState>) -> Result<Json<HealthCheckResponse>> {
    // Check database connectivity
    let db_start = Instant::now();
    let db_result = sqlx::query!("SELECT 1 as one")
        .fetch_one(&state.db)
        .await;
    let db_latency = db_start.elapsed();
    
    let database_health = match db_result {
        Ok(_) => DatabaseHealth {
            connected: true,
            latency_ms: Some(db_latency.as_millis() as u32),
        },
        Err(_) => DatabaseHealth {
            connected: false,
            latency_ms: None,
        },
    };
    
    // Calculate uptime
    let uptime_seconds = SERVER_START
        .get()
        .map(|start| start.elapsed().as_secs())
        .unwrap_or(0);
    
    // Get version from Cargo.toml
    let version = env!("CARGO_PKG_VERSION").to_string();
    
    let response = HealthCheckResponse {
        status: if database_health.connected { "healthy" } else { "degraded" }.to_string(),
        database: database_health,
        uptime_seconds,
        version,
    };
    
    Ok(Json(response))
}

pub async fn health_dashboard(State(_state): State<AppState>) -> Result<axum::response::Html<String>> {
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Automatafl Health Dashboard</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background-color: #1a1a1a;
            color: #e0e0e0;
            margin: 0;
            padding: 20px;
            line-height: 1.6;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
        }
        h1 {
            color: #4CAF50;
            border-bottom: 2px solid #4CAF50;
            padding-bottom: 10px;
        }
        .status-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
            margin-top: 30px;
        }
        .status-card {
            background-color: #2a2a2a;
            border-radius: 8px;
            padding: 20px;
            box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3);
            transition: transform 0.2s;
        }
        .status-card:hover {
            transform: translateY(-2px);
        }
        .status-card h2 {
            margin-top: 0;
            color: #4CAF50;
            font-size: 1.2em;
        }
        .metric {
            display: flex;
            justify-content: space-between;
            padding: 8px 0;
            border-bottom: 1px solid #3a3a3a;
        }
        .metric:last-child {
            border-bottom: none;
        }
        .metric-label {
            color: #888;
        }
        .metric-value {
            font-weight: bold;
        }
        .status-healthy {
            color: #4CAF50;
        }
        .status-degraded {
            color: #ff9800;
        }
        .status-unhealthy {
            color: #f44336;
        }
        .refresh-info {
            text-align: center;
            margin-top: 30px;
            color: #666;
        }
        #last-update {
            font-family: monospace;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🎮 Automatafl Health Dashboard</h1>
        
        <div class="status-grid">
            <div class="status-card">
                <h2>System Status</h2>
                <div class="metric">
                    <span class="metric-label">Overall Status</span>
                    <span class="metric-value" id="overall-status">Loading...</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Version</span>
                    <span class="metric-value" id="version">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Uptime</span>
                    <span class="metric-value" id="uptime">-</span>
                </div>
            </div>
            
            <div class="status-card">
                <h2>Database</h2>
                <div class="metric">
                    <span class="metric-label">Connection</span>
                    <span class="metric-value" id="db-status">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Latency</span>
                    <span class="metric-value" id="db-latency">-</span>
                </div>
            </div>
            
            <div class="status-card">
                <h2>Game Statistics</h2>
                <div class="metric">
                    <span class="metric-label">Active Games</span>
                    <span class="metric-value" id="active-games">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Total Games</span>
                    <span class="metric-value" id="total-games">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Online Players</span>
                    <span class="metric-value" id="online-players">-</span>
                </div>
            </div>
            
            <div class="status-card">
                <h2>Performance</h2>
                <div class="metric">
                    <span class="metric-label">Request Rate</span>
                    <span class="metric-value" id="request-rate">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Error Rate</span>
                    <span class="metric-value" id="error-rate">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">WebSocket Connections</span>
                    <span class="metric-value" id="ws-connections">-</span>
                </div>
            </div>
        </div>
        
        <div class="refresh-info">
            <p>Auto-refreshing every 5 seconds</p>
            <p>Last update: <span id="last-update">-</span></p>
        </div>
    </div>
    
    <script>
        function formatUptime(seconds) {
            const days = Math.floor(seconds / 86400);
            const hours = Math.floor((seconds % 86400) / 3600);
            const minutes = Math.floor((seconds % 3600) / 60);
            
            if (days > 0) {
                return `${days}d ${hours}h ${minutes}m`;
            } else if (hours > 0) {
                return `${hours}h ${minutes}m`;
            } else {
                return `${minutes}m`;
            }
        }
        
        async function updateHealth() {
            try {
                const response = await fetch('/api/health');
                const data = await response.json();
                
                // Update system status
                const statusEl = document.getElementById('overall-status');
                statusEl.textContent = data.status.toUpperCase();
                statusEl.className = 'metric-value status-' + data.status;
                
                document.getElementById('version').textContent = 'v' + data.version;
                document.getElementById('uptime').textContent = formatUptime(data.uptime_seconds);
                
                // Update database status
                const dbStatusEl = document.getElementById('db-status');
                dbStatusEl.textContent = data.database.connected ? 'CONNECTED' : 'DISCONNECTED';
                dbStatusEl.className = 'metric-value ' + (data.database.connected ? 'status-healthy' : 'status-unhealthy');
                
                document.getElementById('db-latency').textContent = 
                    data.database.latency_ms ? data.database.latency_ms + 'ms' : '-';
                
                // Update last update time
                document.getElementById('last-update').textContent = new Date().toLocaleTimeString();
                
                // TODO: Fetch and update game statistics and performance metrics
                // For now, show placeholder data
                document.getElementById('active-games').textContent = Math.floor(Math.random() * 50);
                document.getElementById('total-games').textContent = Math.floor(Math.random() * 1000 + 500);
                document.getElementById('online-players').textContent = Math.floor(Math.random() * 100 + 20);
                document.getElementById('request-rate').textContent = Math.floor(Math.random() * 1000 + 100) + '/min';
                document.getElementById('error-rate').textContent = (Math.random() * 0.5).toFixed(2) + '%';
                document.getElementById('ws-connections').textContent = Math.floor(Math.random() * 50 + 10);
                
            } catch (error) {
                console.error('Failed to fetch health status:', error);
                document.getElementById('overall-status').textContent = 'ERROR';
                document.getElementById('overall-status').className = 'metric-value status-unhealthy';
            }
        }
        
        // Initial update
        updateHealth();
        
        // Update every 5 seconds
        setInterval(updateHealth, 5000);
    </script>
</body>
</html>"#;
    
    Ok(axum::response::Html(html.to_string()))
}
