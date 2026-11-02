//! Real-time Server Telemetry, Prometheus `/metrics`, Lichess-Style Status Snapshot (`/api/status`),
//! and Fog-of-War-Safe Structured Event Tracing (`journald`).
//!
//! Tracks:
//! - **Lean 4 Verified Kernel FFI**: Call counts, total/avg/max/last execution latency in `µs`.
//! - **AI Bot Engine Search**: Per-strategy move counts (`novice`, `tactical`, `minimax`,
//!   `decoupled_uct`, `regret_matching`) and search latency (`avg_us`, `last_us`, `max_us`).
//! - **HTTP & SSE API Telemetry**: Request counts (`2xx`, `4xx`, `5xx`), endpoint counters,
//!   active SSE streams, and rolling latency percentiles (`p50_us`, `p95_us`, `p99_us`, `avg_us`).
//! - **Game & Matchmaking Lifecycle**: Games created, turns resolved, conflict re-entry clashes,
//!   win distribution (`Seat A`, `Seat B`, `Draw`), LRU cache residency/evictions, and disk loads.
//! - **OS / Cgroup v2 / Bounded Disk Containment**: Live reads of `/proc/self/cgroup`,
//!   `/sys/fs/cgroup/.../memory.{current,high,max,peak}`, `cpu.stat`, `pids.{current,max}`,
//!   `/proc/self/status` (`VmRSS`, `VmHWM`), `/proc/loadavg`, open file descriptors, and
//!   persisted game storage usage.
//! - **Rolling 60-Second Time-Series & Sanitized Event Trace**: 1Hz sparkline history and
//!   recent structured events (with secret committed coordinates strictly redacted).

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::bot::BotStrategy;
use crate::persistence::MAX_PERSISTED_GAMES_ON_DISK;

const LATENCY_RING_CAP: usize = 256;
const SERIES_RING_CAP: usize = 60;
const TRACE_RING_CAP: usize = 50;

static GLOBAL_TELEMETRY: OnceLock<ServerTelemetry> = OnceLock::new();

pub fn global_telemetry() -> &'static ServerTelemetry {
    GLOBAL_TELEMETRY.get_or_init(ServerTelemetry::new)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetrySample {
    pub ts_sec: u64,
    pub req_per_sec: u64,
    pub http_avg_us: u64,
    pub lean_avg_us: u64,
    pub bot_avg_us: u64,
    pub rss_bytes: u64,
    pub cpu_pct: f64,
    pub active_games: usize,
    pub active_sse: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub level: &'static str,
    pub kind: &'static str,
    pub game_id: Option<String>,
    pub duration_us: Option<u64>,
    pub summary: String,
}

struct RollingState {
    http_latencies_us: VecDeque<u64>,
    lean_latencies_us: VecDeque<u64>,
    series: VecDeque<TelemetrySample>,
    events: VecDeque<TraceEvent>,
    last_sample_sec: u64,
    last_sample_reqs: u64,
    last_sample_cpu_usec: u64,
    last_sample_wall_us: u64,
    last_cpu_pct: f64,
}

pub struct ServerTelemetry {
    start_instant: Instant,
    start_epoch_ms: u64,
    event_seq: AtomicU64,

    // HTTP counters
    pub http_requests_total: AtomicU64,
    pub http_2xx_total: AtomicU64,
    pub http_4xx_total: AtomicU64,
    pub http_5xx_total: AtomicU64,
    pub http_duration_us_total: AtomicU64,
    pub http_duration_us_last: AtomicU64,
    pub http_duration_us_max: AtomicU64,

    // Endpoint counters
    pub ep_create_game: AtomicU64,
    pub ep_join_game: AtomicU64,
    pub ep_select_piece: AtomicU64,
    pub ep_submit_move: AtomicU64,
    pub ep_commit_move: AtomicU64,
    pub ep_reveal_round: AtomicU64,
    pub ep_resolve_round: AtomicU64,
    pub ep_resign_game: AtomicU64,
    pub ep_get_view: AtomicU64,
    pub ep_list_games: AtomicU64,
    pub ep_matchmaking: AtomicU64,
    pub ep_replay: AtomicU64,

    // SSE counters
    pub active_sse_streams: AtomicU64,
    pub sse_connections_total: AtomicU64,
    pub sse_events_sent_total: AtomicU64,

    // Lean 4 Verified Kernel FFI counters
    pub lean_ffi_calls_total: AtomicU64,
    pub lean_ffi_errors_total: AtomicU64,
    pub lean_ffi_duration_us_total: AtomicU64,
    pub lean_ffi_duration_us_last: AtomicU64,
    pub lean_ffi_duration_us_max: AtomicU64,

    // AI Bot Engine counters
    pub bot_moves_total: AtomicU64,
    pub bot_duration_us_total: AtomicU64,
    pub bot_duration_us_last: AtomicU64,
    pub bot_duration_us_max: AtomicU64,
    pub bot_novice_total: AtomicU64,
    pub bot_tactical_total: AtomicU64,
    pub bot_minimax_total: AtomicU64,
    pub bot_duct_total: AtomicU64,
    pub bot_regret_total: AtomicU64,

    // Game & Cache lifecycle counters
    pub games_created_total: AtomicU64,
    pub turns_resolved_total: AtomicU64,
    pub conflicts_triggered_total: AtomicU64,
    pub games_finished_total: AtomicU64,
    pub wins_seat_a: AtomicU64,
    pub wins_seat_b: AtomicU64,
    pub draws_total: AtomicU64,
    pub cache_evictions_total: AtomicU64,
    pub disk_loads_total: AtomicU64,
    pub disk_saves_total: AtomicU64,
    pub matchmaking_paired_total: AtomicU64,

    rolling: Mutex<RollingState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencyStats {
    pub count: u64,
    pub last_us: u64,
    pub avg_us: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceTelemetry {
    pub memory_current_bytes: u64,
    pub memory_peak_bytes: u64,
    pub memory_high_bytes: u64,
    pub memory_max_bytes: u64,
    pub rss_bytes: u64,
    pub vmsize_bytes: u64,
    pub cpu_usage_usec: u64,
    pub cpu_user_usec: u64,
    pub cpu_system_usec: u64,
    pub cpu_quota_pct: f64,
    pub cpu_percent: f64,
    pub load_avg_1m: f64,
    pub load_avg_5m: f64,
    pub load_avg_15m: f64,
    pub tasks_current: u64,
    pub tasks_max: u64,
    pub open_fds: u64,
    pub max_fds: u64,
    pub disk_used_bytes: u64,
    pub disk_max_bytes: u64,
    pub persisted_files_count: usize,
    pub max_persisted_files: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BotTelemetrySnapshot {
    pub moves_total: u64,
    pub avg_us: u64,
    pub last_us: u64,
    pub max_us: u64,
    pub by_strategy: BotStrategyCounts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BotStrategyCounts {
    pub novice: u64,
    pub tactical: u64,
    pub minimax: u64,
    pub decoupled_uct: u64,
    pub regret_matching: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndpointCounts {
    pub create_game: u64,
    pub join_game: u64,
    pub select_piece: u64,
    pub submit_move: u64,
    pub commit_move: u64,
    pub reveal_round: u64,
    pub resolve_round: u64,
    pub resign_game: u64,
    pub get_view: u64,
    pub list_games: u64,
    pub matchmaking: u64,
    pub replay: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameLifecycleSnapshot {
    pub in_memory_games: usize,
    pub max_in_memory_games: usize,
    pub active_games: usize,
    pub finished_games_in_ram: usize,
    pub lobby_public_games: usize,
    pub matchmaking_queued: usize,
    pub matchmaking_paired_total: u64,
    pub games_created_total: u64,
    pub turns_resolved_total: u64,
    pub conflicts_triggered_total: u64,
    pub games_finished_total: u64,
    pub wins_seat_a: u64,
    pub wins_seat_b: u64,
    pub draws_total: u64,
    pub cache_evictions_total: u64,
    pub disk_loads_total: u64,
    pub disk_saves_total: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub ok: bool,
    pub server_time_ms: u64,
    pub start_epoch_ms: u64,
    pub uptime_secs: u64,
    pub version: &'static str,
    pub engine: &'static str,
    pub resources: ResourceTelemetry,
    pub http_requests_total: u64,
    pub http_2xx_total: u64,
    pub http_4xx_total: u64,
    pub http_5xx_total: u64,
    pub http_latency: LatencyStats,
    pub endpoints: EndpointCounts,
    pub active_sse_streams: u64,
    pub sse_connections_total: u64,
    pub sse_events_sent_total: u64,
    pub lean_ffi_calls_total: u64,
    pub lean_ffi_errors_total: u64,
    pub lean_ffi_latency: LatencyStats,
    pub bot: BotTelemetrySnapshot,
    pub games: GameLifecycleSnapshot,
    pub series: Vec<TelemetrySample>,
    pub recent_events: Vec<TraceEvent>,
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn update_max(atom: &AtomicU64, val: u64) {
    let mut cur = atom.load(Ordering::Relaxed);
    while val > cur {
        match atom.compare_exchange_weak(cur, val, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => cur = actual,
        }
    }
}

impl Default for ServerTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerTelemetry {
    pub fn new() -> Self {
        Self {
            start_instant: Instant::now(),
            start_epoch_ms: now_epoch_ms(),
            event_seq: AtomicU64::new(0),
            http_requests_total: AtomicU64::new(0),
            http_2xx_total: AtomicU64::new(0),
            http_4xx_total: AtomicU64::new(0),
            http_5xx_total: AtomicU64::new(0),
            http_duration_us_total: AtomicU64::new(0),
            http_duration_us_last: AtomicU64::new(0),
            http_duration_us_max: AtomicU64::new(0),
            ep_create_game: AtomicU64::new(0),
            ep_join_game: AtomicU64::new(0),
            ep_select_piece: AtomicU64::new(0),
            ep_submit_move: AtomicU64::new(0),
            ep_commit_move: AtomicU64::new(0),
            ep_reveal_round: AtomicU64::new(0),
            ep_resolve_round: AtomicU64::new(0),
            ep_resign_game: AtomicU64::new(0),
            ep_get_view: AtomicU64::new(0),
            ep_list_games: AtomicU64::new(0),
            ep_matchmaking: AtomicU64::new(0),
            ep_replay: AtomicU64::new(0),
            active_sse_streams: AtomicU64::new(0),
            sse_connections_total: AtomicU64::new(0),
            sse_events_sent_total: AtomicU64::new(0),
            lean_ffi_calls_total: AtomicU64::new(0),
            lean_ffi_errors_total: AtomicU64::new(0),
            lean_ffi_duration_us_total: AtomicU64::new(0),
            lean_ffi_duration_us_last: AtomicU64::new(0),
            lean_ffi_duration_us_max: AtomicU64::new(0),
            bot_moves_total: AtomicU64::new(0),
            bot_duration_us_total: AtomicU64::new(0),
            bot_duration_us_last: AtomicU64::new(0),
            bot_duration_us_max: AtomicU64::new(0),
            bot_novice_total: AtomicU64::new(0),
            bot_tactical_total: AtomicU64::new(0),
            bot_minimax_total: AtomicU64::new(0),
            bot_duct_total: AtomicU64::new(0),
            bot_regret_total: AtomicU64::new(0),
            games_created_total: AtomicU64::new(0),
            turns_resolved_total: AtomicU64::new(0),
            conflicts_triggered_total: AtomicU64::new(0),
            games_finished_total: AtomicU64::new(0),
            wins_seat_a: AtomicU64::new(0),
            wins_seat_b: AtomicU64::new(0),
            draws_total: AtomicU64::new(0),
            cache_evictions_total: AtomicU64::new(0),
            disk_loads_total: AtomicU64::new(0),
            disk_saves_total: AtomicU64::new(0),
            matchmaking_paired_total: AtomicU64::new(0),
            rolling: Mutex::new(RollingState {
                http_latencies_us: VecDeque::with_capacity(LATENCY_RING_CAP),
                lean_latencies_us: VecDeque::with_capacity(LATENCY_RING_CAP),
                series: VecDeque::with_capacity(SERIES_RING_CAP),
                events: VecDeque::with_capacity(TRACE_RING_CAP),
                last_sample_sec: 0,
                last_sample_reqs: 0,
                last_sample_cpu_usec: 0,
                last_sample_wall_us: 0,
                last_cpu_pct: 0.0,
            }),
        }
    }

    pub fn record_lean_ffi(&self, duration_us: u64, ok: bool) {
        let d = duration_us.max(1);
        self.lean_ffi_calls_total.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.lean_ffi_errors_total.fetch_add(1, Ordering::Relaxed);
        }
        self.lean_ffi_duration_us_total
            .fetch_add(d, Ordering::Relaxed);
        self.lean_ffi_duration_us_last.store(d, Ordering::Relaxed);
        update_max(&self.lean_ffi_duration_us_max, d);
        if let Ok(mut g) = self.rolling.try_lock() {
            if g.lean_latencies_us.len() >= LATENCY_RING_CAP {
                g.lean_latencies_us.pop_front();
            }
            g.lean_latencies_us.push_back(d);
        }
    }

    pub fn record_bot_move(&self, strategy: BotStrategy, duration_us: u64) {
        let d = duration_us.max(1);
        self.bot_moves_total.fetch_add(1, Ordering::Relaxed);
        self.bot_duration_us_total.fetch_add(d, Ordering::Relaxed);
        self.bot_duration_us_last.store(d, Ordering::Relaxed);
        update_max(&self.bot_duration_us_max, d);
        match strategy {
            BotStrategy::Novice => self.bot_novice_total.fetch_add(1, Ordering::Relaxed),
            BotStrategy::Tactical => self.bot_tactical_total.fetch_add(1, Ordering::Relaxed),
            BotStrategy::Minimax => self.bot_minimax_total.fetch_add(1, Ordering::Relaxed),
            BotStrategy::DecoupledUct => self.bot_duct_total.fetch_add(1, Ordering::Relaxed),
            BotStrategy::RegretMatching => self.bot_regret_total.fetch_add(1, Ordering::Relaxed),
        };
    }

    pub fn record_http_request(&self, status_code: u16, duration_us: u64) {
        let d = duration_us.max(1);
        self.http_requests_total.fetch_add(1, Ordering::Relaxed);
        match status_code {
            200..=299 => {
                self.http_2xx_total.fetch_add(1, Ordering::Relaxed);
            }
            400..=499 => {
                self.http_4xx_total.fetch_add(1, Ordering::Relaxed);
            }
            500..=599 => {
                self.http_5xx_total.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        self.http_duration_us_total.fetch_add(d, Ordering::Relaxed);
        self.http_duration_us_last.store(d, Ordering::Relaxed);
        update_max(&self.http_duration_us_max, d);
        if let Ok(mut g) = self.rolling.try_lock() {
            if g.http_latencies_us.len() >= LATENCY_RING_CAP {
                g.http_latencies_us.pop_front();
            }
            g.http_latencies_us.push_back(d);
        }
    }

    /// Record a fog-of-war-safe structured event in the in-memory ring buffer and emit a concise
    /// structured line to `stderr` (`journald`).
    pub fn trace_event(
        &self,
        level: &'static str,
        kind: &'static str,
        game_id: Option<&str>,
        duration_us: Option<u64>,
        summary: String,
    ) {
        let seq = self.event_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let ts_ms = now_epoch_ms();
        let ev = TraceEvent {
            seq,
            ts_ms,
            level,
            kind,
            game_id: game_id.map(str::to_owned),
            duration_us,
            summary,
        };

        // Emit structured log line to stderr (picked up by systemd journald) unless silenced in tests
        if std::env::var("AUTOMATAFL_QUIET_LOGS").as_deref() != Ok("1") {
            let gid = ev.game_id.as_deref().unwrap_or("-");
            match ev.duration_us {
                Some(us) => eprintln!(
                    "[{}] kind={} game={} dur_us={} msg=\"{}\"",
                    ev.level, ev.kind, gid, us, ev.summary
                ),
                None => eprintln!(
                    "[{}] kind={} game={} msg=\"{}\"",
                    ev.level, ev.kind, gid, ev.summary
                ),
            }
        }

        if let Ok(mut g) = self.rolling.lock() {
            if g.events.len() >= TRACE_RING_CAP {
                g.events.pop_front();
            }
            g.events.push_back(ev);
        }
    }

    fn compute_percentiles(
        samples: &VecDeque<u64>,
        count: u64,
        total_us: u64,
        last_us: u64,
        max_us: u64,
    ) -> LatencyStats {
        let avg_us = total_us.checked_div(count).unwrap_or(0);
        if samples.is_empty() {
            return LatencyStats {
                count,
                last_us,
                avg_us,
                p50_us: avg_us,
                p95_us: avg_us,
                p99_us: max_us,
                max_us,
            };
        }
        let mut sorted: Vec<u64> = samples.iter().copied().collect();
        sorted.sort_unstable();
        let n = sorted.len();
        let p50_us = sorted[(n * 50 / 100).min(n - 1)];
        let p95_us = sorted[(n * 95 / 100).min(n - 1)];
        let p99_us = sorted[(n * 99 / 100).min(n - 1)];
        LatencyStats {
            count,
            last_us,
            avg_us,
            p50_us,
            p95_us,
            p99_us,
            max_us,
        }
    }

    pub fn snapshot(
        &self,
        in_memory_games: usize,
        max_in_memory_games: usize,
        active_games: usize,
        finished_games_in_ram: usize,
        lobby_public_games: usize,
        matchmaking_queued: usize,
        data_dir: Option<&Path>,
    ) -> StatusSnapshot {
        let uptime_secs = self.start_instant.elapsed().as_secs();
        let wall_us = self.start_instant.elapsed().as_micros() as u64;
        let mut res = read_os_and_cgroup_resources(data_dir);

        let http_count = self.http_requests_total.load(Ordering::Relaxed);
        let http_total_us = self.http_duration_us_total.load(Ordering::Relaxed);
        let http_last_us = self.http_duration_us_last.load(Ordering::Relaxed);
        let http_max_us = self.http_duration_us_max.load(Ordering::Relaxed);

        let lean_count = self.lean_ffi_calls_total.load(Ordering::Relaxed);
        let lean_total_us = self.lean_ffi_duration_us_total.load(Ordering::Relaxed);
        let lean_last_us = self.lean_ffi_duration_us_last.load(Ordering::Relaxed);
        let lean_max_us = self.lean_ffi_duration_us_max.load(Ordering::Relaxed);

        let bot_count = self.bot_moves_total.load(Ordering::Relaxed);
        let bot_total_us = self.bot_duration_us_total.load(Ordering::Relaxed);
        let bot_last_us = self.bot_duration_us_last.load(Ordering::Relaxed);
        let bot_max_us = self.bot_duration_us_max.load(Ordering::Relaxed);
        let bot_avg_us = bot_total_us.checked_div(bot_count).unwrap_or(0);

        let active_sse = self.active_sse_streams.load(Ordering::Relaxed);

        let (http_latency, lean_ffi_latency, series, recent_events) = {
            let mut g = self.rolling.lock().expect("telemetry lock");
            let now_sec = now_epoch_ms() / 1000;
            if now_sec > g.last_sample_sec {
                let dt_sec = (now_sec - g.last_sample_sec).max(1);
                let d_reqs = http_count.saturating_sub(g.last_sample_reqs);
                let rps = if g.last_sample_sec == 0 {
                    0
                } else {
                    d_reqs / dt_sec
                };

                let dt_wall_us = wall_us.saturating_sub(g.last_sample_wall_us);
                let d_cpu_us = res.cpu_usage_usec.saturating_sub(g.last_sample_cpu_usec);
                if g.last_sample_wall_us > 0 && dt_wall_us > 0 {
                    g.last_cpu_pct =
                        ((d_cpu_us as f64 / dt_wall_us as f64) * 100.0).clamp(0.0, 400.0);
                }
                res.cpu_percent = g.last_cpu_pct;

                g.last_sample_sec = now_sec;
                g.last_sample_reqs = http_count;
                g.last_sample_cpu_usec = res.cpu_usage_usec;
                g.last_sample_wall_us = wall_us;

                let http_avg = http_total_us.checked_div(http_count).unwrap_or(0);
                let lean_avg = lean_total_us.checked_div(lean_count).unwrap_or(0);

                if g.series.len() >= SERIES_RING_CAP {
                    g.series.pop_front();
                }
                g.series.push_back(TelemetrySample {
                    ts_sec: now_sec,
                    req_per_sec: rps,
                    http_avg_us: http_avg,
                    lean_avg_us: lean_avg,
                    bot_avg_us,
                    rss_bytes: res.memory_current_bytes,
                    cpu_pct: res.cpu_percent,
                    active_games,
                    active_sse,
                });
            } else {
                res.cpu_percent = g.last_cpu_pct;
            }

            let http_lat = Self::compute_percentiles(
                &g.http_latencies_us,
                http_count,
                http_total_us,
                http_last_us,
                http_max_us,
            );
            let lean_lat = Self::compute_percentiles(
                &g.lean_latencies_us,
                lean_count,
                lean_total_us,
                lean_last_us,
                lean_max_us,
            );
            let ser: Vec<TelemetrySample> = g.series.iter().cloned().collect();
            let evs: Vec<TraceEvent> = g.events.iter().rev().cloned().collect();
            (http_lat, lean_lat, ser, evs)
        };

        StatusSnapshot {
            ok: true,
            server_time_ms: now_epoch_ms(),
            start_epoch_ms: self.start_epoch_ms,
            uptime_secs,
            version: env!("CARGO_PKG_VERSION"),
            engine: "Lean 4.29.0 Verified C IR + PackedBoard64",
            resources: res,
            http_requests_total: http_count,
            http_2xx_total: self.http_2xx_total.load(Ordering::Relaxed),
            http_4xx_total: self.http_4xx_total.load(Ordering::Relaxed),
            http_5xx_total: self.http_5xx_total.load(Ordering::Relaxed),
            http_latency,
            endpoints: EndpointCounts {
                create_game: self.ep_create_game.load(Ordering::Relaxed),
                join_game: self.ep_join_game.load(Ordering::Relaxed),
                select_piece: self.ep_select_piece.load(Ordering::Relaxed),
                submit_move: self.ep_submit_move.load(Ordering::Relaxed),
                commit_move: self.ep_commit_move.load(Ordering::Relaxed),
                reveal_round: self.ep_reveal_round.load(Ordering::Relaxed),
                resolve_round: self.ep_resolve_round.load(Ordering::Relaxed),
                resign_game: self.ep_resign_game.load(Ordering::Relaxed),
                get_view: self.ep_get_view.load(Ordering::Relaxed),
                list_games: self.ep_list_games.load(Ordering::Relaxed),
                matchmaking: self.ep_matchmaking.load(Ordering::Relaxed),
                replay: self.ep_replay.load(Ordering::Relaxed),
            },
            active_sse_streams: active_sse,
            sse_connections_total: self.sse_connections_total.load(Ordering::Relaxed),
            sse_events_sent_total: self.sse_events_sent_total.load(Ordering::Relaxed),
            lean_ffi_calls_total: lean_count,
            lean_ffi_errors_total: self.lean_ffi_errors_total.load(Ordering::Relaxed),
            lean_ffi_latency,
            bot: BotTelemetrySnapshot {
                moves_total: bot_count,
                avg_us: bot_avg_us,
                last_us: bot_last_us,
                max_us: bot_max_us,
                by_strategy: BotStrategyCounts {
                    novice: self.bot_novice_total.load(Ordering::Relaxed),
                    tactical: self.bot_tactical_total.load(Ordering::Relaxed),
                    minimax: self.bot_minimax_total.load(Ordering::Relaxed),
                    decoupled_uct: self.bot_duct_total.load(Ordering::Relaxed),
                    regret_matching: self.bot_regret_total.load(Ordering::Relaxed),
                },
            },
            games: GameLifecycleSnapshot {
                in_memory_games,
                max_in_memory_games,
                active_games,
                finished_games_in_ram,
                lobby_public_games,
                matchmaking_queued,
                matchmaking_paired_total: self.matchmaking_paired_total.load(Ordering::Relaxed),
                games_created_total: self.games_created_total.load(Ordering::Relaxed),
                turns_resolved_total: self.turns_resolved_total.load(Ordering::Relaxed),
                conflicts_triggered_total: self.conflicts_triggered_total.load(Ordering::Relaxed),
                games_finished_total: self.games_finished_total.load(Ordering::Relaxed),
                wins_seat_a: self.wins_seat_a.load(Ordering::Relaxed),
                wins_seat_b: self.wins_seat_b.load(Ordering::Relaxed),
                draws_total: self.draws_total.load(Ordering::Relaxed),
                cache_evictions_total: self.cache_evictions_total.load(Ordering::Relaxed),
                disk_loads_total: self.disk_loads_total.load(Ordering::Relaxed),
                disk_saves_total: self.disk_saves_total.load(Ordering::Relaxed),
            },
            series,
            recent_events,
        }
    }

    pub fn render_prometheus_metrics(&self, snap: &StatusSnapshot) -> String {
        let mut out = String::with_capacity(4096);
        let r = &snap.resources;
        let g = &snap.games;
        out.push_str("# HELP automatafl_up Whether the Automatafl fog-of-war server is healthy.\n");
        out.push_str("# TYPE automatafl_up gauge\n");
        out.push_str("automatafl_up 1\n");

        out.push_str("# HELP automatafl_uptime_seconds Server uptime in seconds.\n");
        out.push_str("# TYPE automatafl_uptime_seconds counter\n");
        out.push_str(&format!("automatafl_uptime_seconds {}\n", snap.uptime_secs));

        out.push_str("# HELP automatafl_memory_current_bytes Current cgroup/RSS memory usage in bytes.\n");
        out.push_str("# TYPE automatafl_memory_current_bytes gauge\n");
        out.push_str(&format!("automatafl_memory_current_bytes {}\n", r.memory_current_bytes));

        out.push_str("# HELP automatafl_memory_peak_bytes Peak cgroup/RSS memory usage in bytes.\n");
        out.push_str("# TYPE automatafl_memory_peak_bytes gauge\n");
        out.push_str(&format!("automatafl_memory_peak_bytes {}\n", r.memory_peak_bytes));

        out.push_str("# HELP automatafl_memory_max_bytes Hard cgroup memory cap in bytes.\n");
        out.push_str("# TYPE automatafl_memory_max_bytes gauge\n");
        out.push_str(&format!("automatafl_memory_max_bytes {}\n", r.memory_max_bytes));

        out.push_str("# HELP automatafl_cpu_usage_microseconds_total Cumulative CPU time in microseconds.\n");
        out.push_str("# TYPE automatafl_cpu_usage_microseconds_total counter\n");
        out.push_str(&format!("automatafl_cpu_usage_microseconds_total {}\n", r.cpu_usage_usec));

        out.push_str("# HELP automatafl_cpu_percent Current rolling CPU utilization percentage.\n");
        out.push_str("# TYPE automatafl_cpu_percent gauge\n");
        out.push_str(&format!("automatafl_cpu_percent {:.2}\n", r.cpu_percent));

        out.push_str("# HELP automatafl_open_fds Open file descriptors.\n");
        out.push_str("# TYPE automatafl_open_fds gauge\n");
        out.push_str(&format!("automatafl_open_fds {}\n", r.open_fds));

        out.push_str("# HELP automatafl_disk_used_bytes Bytes stored in persistent game directory.\n");
        out.push_str("# TYPE automatafl_disk_used_bytes gauge\n");
        out.push_str(&format!("automatafl_disk_used_bytes {}\n", r.disk_used_bytes));

        out.push_str("# HELP automatafl_disk_max_bytes Bounded filesystem capacity for persistent game store.\n");
        out.push_str("# TYPE automatafl_disk_max_bytes gauge\n");
        out.push_str(&format!("automatafl_disk_max_bytes {}\n", r.disk_max_bytes));

        out.push_str("# HELP automatafl_http_requests_total Total HTTP requests processed.\n");
        out.push_str("# TYPE automatafl_http_requests_total counter\n");
        out.push_str(&format!("automatafl_http_requests_total{{status=\"2xx\"}} {}\n", snap.http_2xx_total));
        out.push_str(&format!("automatafl_http_requests_total{{status=\"4xx\"}} {}\n", snap.http_4xx_total));
        out.push_str(&format!("automatafl_http_requests_total{{status=\"5xx\"}} {}\n", snap.http_5xx_total));

        out.push_str("# HELP automatafl_http_request_duration_microseconds Rolling HTTP latency percentiles.\n");
        out.push_str("# TYPE automatafl_http_request_duration_microseconds gauge\n");
        out.push_str(&format!("automatafl_http_request_duration_microseconds{{quantile=\"0.5\"}} {}\n", snap.http_latency.p50_us));
        out.push_str(&format!("automatafl_http_request_duration_microseconds{{quantile=\"0.95\"}} {}\n", snap.http_latency.p95_us));
        out.push_str(&format!("automatafl_http_request_duration_microseconds{{quantile=\"0.99\"}} {}\n", snap.http_latency.p99_us));

        out.push_str("# HELP automatafl_lean_ffi_calls_total Total calls into the formally verified Lean 4 C kernel.\n");
        out.push_str("# TYPE automatafl_lean_ffi_calls_total counter\n");
        out.push_str(&format!("automatafl_lean_ffi_calls_total {}\n", snap.lean_ffi_calls_total));

        out.push_str("# HELP automatafl_lean_ffi_duration_microseconds Rolling Lean 4 FFI call duration in microseconds.\n");
        out.push_str("# TYPE automatafl_lean_ffi_duration_microseconds gauge\n");
        out.push_str(&format!("automatafl_lean_ffi_duration_microseconds{{quantile=\"0.5\"}} {}\n", snap.lean_ffi_latency.p50_us));
        out.push_str(&format!("automatafl_lean_ffi_duration_microseconds{{quantile=\"0.95\"}} {}\n", snap.lean_ffi_latency.p95_us));
        out.push_str(&format!("automatafl_lean_ffi_duration_microseconds{{quantile=\"0.99\"}} {}\n", snap.lean_ffi_latency.p99_us));

        out.push_str("# HELP automatafl_bot_moves_total Server-hosted AI bot moves computed by strategy.\n");
        out.push_str("# TYPE automatafl_bot_moves_total counter\n");
        out.push_str(&format!("automatafl_bot_moves_total{{strategy=\"novice\"}} {}\n", snap.bot.by_strategy.novice));
        out.push_str(&format!("automatafl_bot_moves_total{{strategy=\"tactical\"}} {}\n", snap.bot.by_strategy.tactical));
        out.push_str(&format!("automatafl_bot_moves_total{{strategy=\"minimax\"}} {}\n", snap.bot.by_strategy.minimax));
        out.push_str(&format!("automatafl_bot_moves_total{{strategy=\"decoupled_uct\"}} {}\n", snap.bot.by_strategy.decoupled_uct));
        out.push_str(&format!("automatafl_bot_moves_total{{strategy=\"regret_matching\"}} {}\n", snap.bot.by_strategy.regret_matching));

        out.push_str("# HELP automatafl_games_in_memory Current game sessions resident in RAM LRU cache.\n");
        out.push_str("# TYPE automatafl_games_in_memory gauge\n");
        out.push_str(&format!("automatafl_games_in_memory {}\n", g.in_memory_games));

        out.push_str("# HELP automatafl_games_on_disk Persisted game JSON snapshots on disk.\n");
        out.push_str("# TYPE automatafl_games_on_disk gauge\n");
        out.push_str(&format!("automatafl_games_on_disk {}\n", r.persisted_files_count));

        out.push_str("# HELP automatafl_sse_streams_active Active SSE live-game subscribers.\n");
        out.push_str("# TYPE automatafl_sse_streams_active gauge\n");
        out.push_str(&format!("automatafl_sse_streams_active {}\n", snap.active_sse_streams));

        out.push_str("# HELP automatafl_turns_resolved_total Total simultaneous-move turns resolved.\n");
        out.push_str("# TYPE automatafl_turns_resolved_total counter\n");
        out.push_str(&format!("automatafl_turns_resolved_total {}\n", g.turns_resolved_total));

        out.push_str("# HELP automatafl_conflicts_triggered_total Total simultaneous-move conflict clashes.\n");
        out.push_str("# TYPE automatafl_conflicts_triggered_total counter\n");
        out.push_str(&format!("automatafl_conflicts_triggered_total {}\n", g.conflicts_triggered_total));

        out
    }
}

fn read_u64_file(path: impl AsRef<Path>) -> Option<u64> {
    let s = fs::read_to_string(path).ok()?;
    s.trim().parse::<u64>().ok()
}

fn read_os_and_cgroup_resources(data_dir: Option<&Path>) -> ResourceTelemetry {
    let mut rss_bytes = 0u64;
    let mut hwm_bytes = 0u64;
    let mut vmsize_bytes = 0u64;
    let mut threads = 1u64;

    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                if let Some(kb) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                    rss_bytes = kb * 1024;
                }
            } else if let Some(rest) = line.strip_prefix("VmHWM:") {
                if let Some(kb) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                    hwm_bytes = kb * 1024;
                }
            } else if let Some(rest) = line.strip_prefix("VmSize:") {
                if let Some(kb) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                    vmsize_bytes = kb * 1024;
                }
            } else if let Some(rest) = line.strip_prefix("Threads:") {
                if let Some(t) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                    threads = t;
                }
            }
        }
    }

    // Locate cgroup v2 directory from /proc/self/cgroup (e.g., 0::/automatafl.slice/automatafl.service)
    let mut cgroup_dir = Path::new("/sys/fs/cgroup").to_path_buf();
    let mut slice_dir = Path::new("/sys/fs/cgroup").to_path_buf();
    if let Ok(cg) = fs::read_to_string("/proc/self/cgroup") {
        for line in cg.lines() {
            if let Some(rel) = line.strip_prefix("0::/") {
                let rel_trimmed = rel.trim();
                if !rel_trimmed.is_empty() {
                    cgroup_dir = Path::new("/sys/fs/cgroup").join(rel_trimmed);
                    if let Some(parent) = cgroup_dir.parent() {
                        slice_dir = parent.to_path_buf();
                    }
                }
            }
        }
    }

    let memory_current_bytes = read_u64_file(cgroup_dir.join("memory.current"))
        .or_else(|| read_u64_file("/sys/fs/cgroup/memory.current"))
        .unwrap_or(rss_bytes);
    let memory_peak_bytes = read_u64_file(cgroup_dir.join("memory.peak"))
        .or_else(|| read_u64_file("/sys/fs/cgroup/memory.peak"))
        .unwrap_or(hwm_bytes.max(memory_current_bytes));
    let memory_max_bytes = read_u64_file(cgroup_dir.join("memory.max"))
        .or_else(|| read_u64_file(slice_dir.join("memory.max")))
        .unwrap_or(256 * 1024 * 1024);
    let memory_high_bytes = read_u64_file(slice_dir.join("memory.high"))
        .or_else(|| read_u64_file(cgroup_dir.join("memory.high")))
        .unwrap_or(192 * 1024 * 1024);

    let mut cpu_usage_usec = 0u64;
    let mut cpu_user_usec = 0u64;
    let mut cpu_system_usec = 0u64;
    if let Ok(cpustat) = fs::read_to_string(cgroup_dir.join("cpu.stat"))
        .or_else(|_| fs::read_to_string("/sys/fs/cgroup/cpu.stat"))
    {
        for line in cpustat.lines() {
            let mut parts = line.split_whitespace();
            match (parts.next(), parts.next().and_then(|v| v.parse::<u64>().ok())) {
                (Some("usage_usec"), Some(v)) => cpu_usage_usec = v,
                (Some("user_usec"), Some(v)) => cpu_user_usec = v,
                (Some("system_usec"), Some(v)) => cpu_system_usec = v,
                _ => {}
            }
        }
    } else if let Ok(stat) = fs::read_to_string("/proc/self/stat") {
        // Fallback: /proc/self/stat fields 14 (utime) and 15 (stime) in USER_HZ (100Hz = 10_000us/tick)
        if let Some(after_comm) = stat.rfind(')').map(|i| &stat[i + 1..]) {
            let toks: Vec<&str> = after_comm.split_whitespace().collect();
            if toks.len() > 12 {
                let utime = toks[11].parse::<u64>().unwrap_or(0);
                let stime = toks[12].parse::<u64>().unwrap_or(0);
                cpu_user_usec = utime * 10_000;
                cpu_system_usec = stime * 10_000;
                cpu_usage_usec = cpu_user_usec + cpu_system_usec;
            }
        }
    }

    let mut cpu_quota_pct = 100.0;
    if let Ok(cpumax) = fs::read_to_string(slice_dir.join("cpu.max"))
        .or_else(|_| fs::read_to_string(cgroup_dir.join("cpu.max")))
    {
        let mut parts = cpumax.split_whitespace();
        if let (Some(q_str), Some(p_str)) = (parts.next(), parts.next()) {
            if let (Ok(q), Ok(p)) = (q_str.parse::<f64>(), p_str.parse::<f64>()) {
                if p > 0.0 {
                    cpu_quota_pct = (q / p) * 100.0;
                }
            }
        }
    }

    let mut load_avg_1m = 0.0;
    let mut load_avg_5m = 0.0;
    let mut load_avg_15m = 0.0;
    if let Ok(la) = fs::read_to_string("/proc/loadavg") {
        let mut parts = la.split_whitespace();
        load_avg_1m = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        load_avg_5m = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        load_avg_15m = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    }

    let tasks_current = read_u64_file(cgroup_dir.join("pids.current")).unwrap_or(threads);
    let tasks_max = read_u64_file(cgroup_dir.join("pids.max"))
        .or_else(|| read_u64_file(slice_dir.join("pids.max")))
        .unwrap_or(128);

    let open_fds = fs::read_dir("/proc/self/fd")
        .map(|it| it.count() as u64)
        .unwrap_or(0);
    let mut max_fds = 2048u64;
    if let Ok(limits) = fs::read_to_string("/proc/self/limits") {
        for line in limits.lines() {
            if let Some(rest) = line.strip_prefix("Max open files") {
                if let Some(soft) = rest.split_whitespace().next().and_then(|s| s.parse::<u64>().ok()) {
                    max_fds = soft;
                }
            }
        }
    }

    let mut disk_used_bytes = 0u64;
    let mut persisted_files_count = 0usize;
    if let Some(dir) = data_dir {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        disk_used_bytes += meta.len();
                        let is_json = entry
                            .file_name()
                            .to_str()
                            .is_some_and(|n| !n.starts_with('.') && n.ends_with(".json"));
                        if is_json {
                            persisted_files_count += 1;
                        }
                    }
                }
            }
        }
    }

    ResourceTelemetry {
        memory_current_bytes,
        memory_peak_bytes,
        memory_high_bytes,
        memory_max_bytes,
        rss_bytes: if rss_bytes > 0 { rss_bytes } else { memory_current_bytes },
        vmsize_bytes,
        cpu_usage_usec,
        cpu_user_usec,
        cpu_system_usec,
        cpu_quota_pct,
        cpu_percent: 0.0,
        load_avg_1m,
        load_avg_5m,
        load_avg_15m,
        tasks_current,
        tasks_max,
        open_fds,
        max_fds,
        disk_used_bytes,
        disk_max_bytes: 55 * 1024 * 1024, // 55 MiB ext4 filesystem on 64 MiB loop image
        persisted_files_count,
        max_persisted_files: MAX_PERSISTED_GAMES_ON_DISK,
    }
}
