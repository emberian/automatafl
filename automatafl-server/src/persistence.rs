//! Durable game persistence and replay verification for Automatafl (`R5`).
//!
//! Stores game sessions as atomic JSON snapshots on disk (`<data_dir>/<game_id>.json`)
//! and verifies replay integrity (`GameSession::verify_replay`) on load.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::session::GameSession;
use crate::telemetry::global_telemetry;

/// Hard upper bound on persisted game JSON files on disk (~1 KB each -> < 4 MiB total).
pub const MAX_PERSISTED_GAMES_ON_DISK: usize = 2048;

/// Maximum number of recent games eagerly loaded into RAM on server boot (older games remain
/// on disk and are transparently lazy-loaded on demand via `AppState::get_or_load_session`).
pub const MAX_STARTUP_LOAD_GAMES: usize = 256;

static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct GameStore {
    root: Option<PathBuf>,
}

impl GameStore {
    /// In-memory-only store (does not write files to disk).
    pub fn memory() -> Self {
        Self { root: None }
    }

    /// Durable disk-backed store rooted at `dir`. Creates `dir` if it does not exist.
    pub fn disk(dir: impl AsRef<Path>) -> Result<Self, String> {
        let root = dir.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|e| format!("failed to create persistence dir {}: {e}", root.display()))?;
        Ok(Self { root: Some(root) })
    }

    pub fn is_persistent(&self) -> bool {
        self.root.is_some()
    }

    pub fn root_dir(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn game_path(&self, root: &Path, game_id: &str) -> Result<PathBuf, String> {
        // Sanitize game_id to prevent path traversal.
        if game_id.is_empty()
            || !game_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("invalid game_id for persistence: {game_id:?}"));
        }
        Ok(root.join(format!("{game_id}.json")))
    }

    /// Save a `GameSession` atomically (write to `.tmp` then rename).
    pub fn save_game(&self, session: &GameSession) -> Result<(), String> {
        let Some(ref root) = self.root else {
            return Ok(());
        };
        let target = self.game_path(root, &session.id)?;
        let tmp = root.join(format!(".{}.json.tmp", session.id));
        let payload = serde_json::to_vec(session)
            .map_err(|e| format!("failed to serialize game {}: {e}", session.id))?;
        if let Err(e) = fs::write(&tmp, &payload) {
            self.prune_excess_disk_files(root);
            fs::write(&tmp, &payload)
                .map_err(|e2| format!("failed to write temp snapshot {} ({e}): {e2}", tmp.display()))?;
        }
        fs::rename(&tmp, &target).map_err(|e| {
            format!(
                "failed to atomically rename {} -> {}: {e}",
                tmp.display(),
                target.display()
            )
        })?;
        global_telemetry()
            .disk_saves_total
            .fetch_add(1, Ordering::Relaxed);
        if SAVE_COUNTER.fetch_add(1, Ordering::Relaxed).is_multiple_of(64) {
            self.prune_excess_disk_files(root);
        }
        Ok(())
    }

    fn prune_excess_disk_files(&self, root: &Path) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.starts_with('.') && name.ends_with(".tmp") {
                let _ = fs::remove_file(&path);
                continue;
            }
            if name.starts_with('.') || !name.ends_with(".json") {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((path, mtime));
        }
        if files.len() > MAX_PERSISTED_GAMES_ON_DISK {
            files.sort_by_key(|(_, mtime)| *mtime);
            let to_remove = files.len() - MAX_PERSISTED_GAMES_ON_DISK;
            for (path, _) in files.into_iter().take(to_remove) {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// Load a single game from disk and verify its turn history replay.
    pub fn load_game(&self, game_id: &str) -> Result<Option<GameSession>, String> {
        let Some(ref root) = self.root else {
            return Ok(None);
        };
        let path = self.game_path(root, game_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes =
            fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let session: GameSession = serde_json::from_slice(&bytes)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
        session
            .verify_replay()
            .map_err(|e| format!("replay verification failed for {}: {e}", session.id))?;
        global_telemetry()
            .disk_loads_total
            .fetch_add(1, Ordering::Relaxed);
        Ok(Some(session))
    }

    /// Load up to `MAX_STARTUP_LOAD_GAMES` most recently modified persisted games from the directory
    /// and verify every loaded game's replay integrity (older games remain on disk for lazy loading).
    pub fn load_all_and_verify(&self) -> Result<Vec<GameSession>, String> {
        let Some(ref root) = self.root else {
            return Ok(Vec::new());
        };
        let entries = fs::read_dir(root)
            .map_err(|e| format!("failed to read persistence dir {}: {e}", root.display()))?;
        let mut candidates: Vec<(PathBuf, SystemTime)> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("dir entry error: {e}"))?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.starts_with('.') || !name.ends_with(".json") {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            candidates.push((path, mtime));
        }
        if candidates.len() > MAX_STARTUP_LOAD_GAMES {
            candidates.sort_by(|a, b| b.1.cmp(&a.1));
            candidates.truncate(MAX_STARTUP_LOAD_GAMES);
        }
        let mut sessions = Vec::with_capacity(candidates.len());
        for (path, _) in candidates {
            let bytes =
                fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
            let session: GameSession = serde_json::from_slice(&bytes)
                .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
            session
                .verify_replay()
                .map_err(|e| format!("replay verification failed for {}: {e}", session.id))?;
            sessions.push(session);
        }
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(sessions)
    }
}
