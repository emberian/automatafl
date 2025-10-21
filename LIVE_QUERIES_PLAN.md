# SurrealDB Live Queries Migration Plan

## Current Architecture (Complex)

### Components
1. **In-memory state**: `DashMap<Uuid, broadcast::Sender<GameEventData>>` in `AppState`
2. **Manual broadcasting**: `broadcast_event()` function called after every DB write
3. **WebSocket handler**: Complex `handle_socket()` function manages subscriptions
4. **Cleanup task**: Background task to remove stale game channels
5. **Event flow**:
   - Service writes to `game_events` table
   - Handler calls `broadcast_event()`
   - `broadcast_event()` sends to in-memory channel
   - WebSocket handler forwards to clients

### Problems
- ❌ State is split between DB and memory
- ❌ Events can be lost if server restarts
- ❌ Complex synchronization between DB writes and broadcasts
- ❌ Memory leaks if channels aren't cleaned up
- ❌ Doesn't scale across multiple backend instances

---

## New Architecture (Simple)

### Core Concept
**The database becomes the single source of truth AND the pub/sub system.**

### Event Flow
1. Client connects to `/ws/game/{game_id}`
2. Backend authenticates client
3. Backend opens WebSocket to SurrealDB
4. Backend issues: `LIVE SELECT * FROM game_events WHERE game_id = $game_id ORDER BY timestamp`
5. Service writes event → SurrealDB auto-pushes to all subscribers
6. Backend proxies event to client

### Benefits
- ✅ Database is single source of truth
- ✅ Events persist across restarts
- ✅ No manual synchronization needed
- ✅ No memory management for channels
- ✅ Scales across multiple backend instances
- ✅ Can replay event history (useful for reconnections)

---

## Implementation Steps

### Phase 1: Setup SurrealDB Live Query Support
**Files to modify:**
- `backend/Cargo.toml` - Ensure surrealdb crate has WebSocket support
- `backend/src/db.rs` - Add live query initialization

```rust
// In db.rs
pub async fn create_live_query(
    db: &Surreal<Client>,
    game_id: Uuid,
) -> Result<impl Stream<Item = Notification<GameEventRecord>>, surrealdb::Error> {
    let mut stream = db
        .query("LIVE SELECT * FROM game_events WHERE game_id = $game_id ORDER BY timestamp")
        .bind(("game_id", game_id.to_string()))
        .await?
        .stream::<Notification<GameEventRecord>>(0)?;

    Ok(stream)
}
```

### Phase 2: Refactor WebSocket Handler
**Files to modify:**
- `backend/src/common.rs` - Simplify `handle_socket()`

**Before (70+ lines):**
```rust
pub async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>, game_id: Uuid, player_id: Uuid) {
    // Complex channel management
    // Manual subscription/unsubscription
    // Cleanup on disconnect
}
```

**After (20 lines):**
```rust
pub async fn handle_socket(socket: WebSocket, app_state: Arc<AppState>, game_id: Uuid, player_id: Uuid) {
    let (ws_sink, ws_stream) = socket.split();

    // Create live query stream from SurrealDB
    let db_stream = db::create_live_query(&app_state.db, game_id)
        .await
        .expect("Failed to create live query");

    // Proxy SurrealDB events to WebSocket client
    let forward = db_stream.map(|notification| {
        match notification {
            Notification::Create(event) | Notification::Update(event) => {
                Message::Text(serde_json::to_string(&event.data).unwrap())
            }
            _ => Message::Text("".to_string()),
        }
    }).forward(ws_sink);

    // Handle incoming messages from client (if needed)
    let receive = ws_stream.for_each(|msg| async { /* handle client messages */ });

    tokio::select! {
        _ = forward => {},
        _ = receive => {},
    }
    // Cleanup is automatic - no manual channel removal needed
}
```

### Phase 3: Remove Broadcast System
**Files to modify:**
- `backend/src/common.rs`

**Delete:**
- `game_channels: DashMap<Uuid, broadcast::Sender<GameEventData>>`
- `broadcast_event()` function
- Channel cleanup task in `main.rs`

**In handlers (game.rs, html.rs):**
```rust
// BEFORE: After service call
for event in events {
    broadcast_event(&app_state, game_id, event).await?;
}

// AFTER: Service already wrote to DB, SurrealDB pushes automatically
// No broadcasting needed! Just return the response.
```

### Phase 4: Enhanced Event Payloads (Optional)
Now that events are the single source of truth, enhance them:

```rust
// In api_types/src/lib.rs
pub enum GameEventData {
    GameStarted {
        lifecycle: GameLifecycle, // ← Include new state
    },
    PlayerJoined {
        player_id: Uuid,
        displayname: String,
        all_players: Vec<PlayerInfo>, // ← Include updated list
    },
    MoveAcknowledged {
        player_pid: Pid,
        from: Coord,
        to: Coord,
        pending_moves: Vec<Move>, // ← Include all pending moves
    },
    RoundComplete {
        new_round: u32,           // ← Include round number
        lifecycle: GameLifecycle, // ← Include state if needed
    },
    // ... etc
}
```

**Frontend benefit:** Remove ALL `bump_history` calls, update state directly from events.

---

## Migration Checklist

### Backend Changes
- [ ] Update `Cargo.toml` for SurrealDB WebSocket support
- [ ] Implement `db::create_live_query()`
- [ ] Refactor `common::handle_socket()` to use live queries
- [ ] Remove `broadcast_event()` function
- [ ] Remove `game_channels` from `AppState`
- [ ] Remove channel cleanup task from `main.rs`
- [ ] Remove `broadcast_event()` calls from `game.rs` handlers
- [ ] Remove `broadcast_event()` calls from `html.rs` handlers
- [ ] Update `GameEventData` variants to include state deltas (optional)

### Frontend Changes (after backend is done)
- [ ] Test that WebSocket still receives events
- [ ] Remove `bump_history()` function
- [ ] Remove `history_version` signal
- [ ] Update event handlers to use event payload data directly

### Testing
- [ ] Verify events still arrive in real-time
- [ ] Test multiple clients connected to same game
- [ ] Test backend restart (events should persist)
- [ ] Test reconnection (can replay events if needed)
- [ ] Load test (compare performance vs old broadcast system)

---

## Risks & Mitigations

### Risk 1: SurrealDB WebSocket Overhead
**Concern:** One DB WebSocket per client connection?
**Mitigation:**
- SurrealDB is designed for this (it's a real-time DB)
- Connection pooling may help
- Monitor resource usage in production

### Risk 2: Event Ordering
**Concern:** Are live query results guaranteed to be in order?
**Mitigation:**
- SurrealDB respects ORDER BY in LIVE queries
- Events have timestamps as fallback

### Risk 3: Replay/Reconnection
**Concern:** If client disconnects, do they miss events?
**Mitigation:**
- Track last received event timestamp on client
- On reconnect, fetch missed events via HTTP first, then start live query
- Or: Use SurrealDB's event log (if it supports resuming from timestamp)

---

## Expected Code Reduction

### Lines Removed
- `game_channels` and related: ~30 lines
- `broadcast_event()`: ~20 lines
- Complex `handle_socket()`: ~50 lines
- Channel cleanup task: ~15 lines
- `broadcast_event()` calls in handlers: ~40 lines (across all handlers)
- **Total: ~155 lines deleted**

### Lines Added
- `create_live_query()`: ~15 lines
- Simplified `handle_socket()`: ~20 lines
- **Total: ~35 lines added**

### Net Reduction: **~120 lines**
### Complexity Reduction: **Massive** (eliminates entire subsystem)

---

## Next Steps

1. **Prototype** live queries in a branch
2. **Verify** SurrealDB supports WebSocket LIVE SELECT
3. **Test** with 2-3 clients connected to same game
4. **Benchmark** performance vs current system
5. **Deploy** to staging
6. **Monitor** for 1 week before production

---

## Alternative: Hybrid Approach

If full live queries are too risky:

**Option A: Live Queries for Events, Keep HTTP for State**
- Use live queries ONLY for `game_events`
- Keep HTTP endpoints for fetching full `GameStateResponse`
- Compromise: Simpler than current, but not as elegant

**Option B: Server-Sent Events (SSE) Instead of WebSocket**
- Backend polls SurrealDB, pushes via SSE
- Simpler than WebSocket (one-way)
- Still eliminates in-memory broadcast channels

---

## References

- [SurrealDB Live Queries Docs](https://surrealdb.com/docs/surrealql/statements/live)
- [Axum WebSocket Example](https://github.com/tokio-rs/axum/tree/main/examples/websockets)
- Code Review Section: "Major Improvement 2: Use Live Queries for Real-Time Events"
