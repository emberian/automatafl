//! Automatafl Multi-Target Client Library (`R6`).
//!
//! Provides:
//! - `tui`: Pure deterministic Unicode/ANSI board & HUD renderer for any `GameView`.
//! - `app`: Interactive client state machine (`ClientApp`) supporting AI Bot matches,
//!   Local Fog-of-War Pass-and-Play, and live R2 `GameConfig` variant configuration.
//! - `remote`: Async HTTP/1.1 client (`RemoteClient`) for connecting to `automatafl-server`.

pub mod app;
pub mod remote;
pub mod tui;
