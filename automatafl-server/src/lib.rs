//! Automatafl Fog-of-War Simultaneous-Move Platform Server, Matchmaking & Persistence (`R5` / `R6`).
//!
//! Exposes:
//! - `protocol`: Wire types, board representations, and R2 `GameConfig` variant models.
//! - `oracle`: Wire-protocol rule oracle (`rules_ffi` + Lean 4 FFI hook + R2 variants).
//! - `bot`: Simultaneous-move AI bot opponents (`Novice`, `Tactical`, `Minimax`, `DecoupledUct`,
//!   `RegretMatching` + `automatafl-ai` engine hook) strictly isolated from secret human moves.
//! - `session`: Fog-of-war commit-phase state machine (`Commit`, `Resubmit`, `Reveal`, `Resolve`,
//!   `Over`), constant-time seat tokens, blind hash commitments, conflict re-entry (`marks`,
//!   `locked`, `waiting`), replay verification, and viewer-scoped `GameView` redaction.
//! - `matchmaking`: Automatic matchmaking queue and compatible-variant pairing.
//! - `persistence`: Atomic disk snapshot persistence and replay verification on startup.
//! - `api`: Axum REST + SSE router with strict per-subscriber fog-of-war isolation.
//! - `web_ui`: Responsive HTML5 + CSS Grid + SSE single-page browser client.

pub mod api;
pub mod bot;
pub mod matchmaking;
pub mod oracle;
pub mod persistence;
pub mod protocol;
pub mod session;
pub mod telemetry;
pub mod web_ui;
