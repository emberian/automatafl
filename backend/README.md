# AutomataFL Backend

A production-ready Rust backend for the AutomataFL multiplayer board game, built with Axum, SurrealDB, and real-time WebSocket support.

## 🏗️ Architecture

### Tech Stack
- **Web Framework**: Axum 0.8 with async/await
- **Database**: SurrealDB (embedded RocksDB or external)
- **Authentication**: Argon2id password hashing with session-based auth
- **Real-time**: WebSocket connections with broadcast channels
- **Observability**: OpenTelemetry (metrics & tracing)
- **Templating**: Askama for server-side rendering

### Key Design Principles
- **Persistence-first**: All game state stored in database, in-memory only for real-time events
- **Lock-free concurrency**: DashMap for per-game channels, database handles its own locking
- **Type safety**: Shared types between backend and webapp via `api-types` crate
- **Production-ready**: Session expiration, admin controls, comprehensive error handling

## 🚀 Features

### Core Game Management
- ✅ Game creation with configurable rules (player count, column rule)
- ✅ Real-time game state via WebSocket
- ✅ Move validation and conflict resolution
- ✅ Automaton movement and win detection
- ✅ Save/load game snapshots
- ✅ Complete game history with timestamped events

### User System
- ✅ Secure registration with Argon2id password hashing
- ✅ Session-based authentication (24-hour expiration)
- ✅ User profiles with bio and avatar
- ✅ Player statistics (games played, win rate, total playtime)
- ✅ ELO rating system

### Social Features
- ✅ In-game chat with persistence
- ✅ Leaderboards (ELO, wins, games played)
- ✅ Player profiles viewable by anyone

### Matchmaking
- ✅ Queue-based matchmaking with game preferences
- ✅ Background task matching players every 5 seconds
- ✅ ELO-based matching (configurable tolerance)
- ✅ Auto-game creation when players match

### Admin Features
- ✅ List all players
- ✅ List all games
- ✅ Delete games
- ✅ Force complete game rounds

## 📡 API Endpoints

### Public Endpoints
```
POST   /api/health               - Health check
POST   /api/v1/register          - Register new player
POST   /api/v1/login             - Login and get session token
```

### Authenticated Endpoints

#### Session Management
```
POST   /api/v1/logout            - End session
```

#### Game Management
```
GET    /api/v1/games             - List all games
POST   /api/v1/games             - Create new game
GET    /api/v1/games/{id}        - Get game state
POST   /api/v1/games/{id}        - Join game
GET    /api/v1/games/{id}/goals  - Get game goals
GET    /api/v1/games/{id}/ws     - WebSocket connection for real-time updates
```

#### Game Actions
```
GET    /api/v1/games/{id}/move          - Get pending move
POST   /api/v1/games/{id}/move          - Submit move
POST   /api/v1/games/{id}/complete      - Complete round (if ready)
```

#### Chat & History
```
GET    /api/v1/games/{id}/chat          - Get chat messages
POST   /api/v1/games/{id}/chat          - Send chat message
GET    /api/v1/games/{id}/history       - Get game event history
       Query params: ?since=<ts>&until=<ts>&event_kind=<kind>
```

#### Snapshots
```
POST   /api/v1/games/{id}/save          - Save game snapshot
GET    /api/v1/games/{id}/snapshots     - List snapshots
POST   /api/v1/games/{id}/load/{index}  - Load snapshot
```

#### Profiles
```
GET    /api/v1/players/{id}             - Get player profile
PUT    /api/v1/players/{id}             - Update own profile
GET    /api/v1/players/{id}/stats       - Get player statistics
```

#### Leaderboards
```
GET    /api/v1/leaderboard/elo          - Top players by ELO
GET    /api/v1/leaderboard/wins         - Top players by wins
GET    /api/v1/leaderboard/games        - Most active players
```

#### Matchmaking
```
POST   /api/v1/matchmaking/join         - Join matchmaking queue
POST   /api/v1/matchmaking/leave        - Leave queue
GET    /api/v1/matchmaking/status       - Get queue status
```

#### Admin Endpoints (requires admin role)
```
GET    /api/v1/admin/players            - List all players
GET    /api/v1/admin/games              - List all games
DELETE /api/v1/admin/games/{id}         - Delete game
POST   /api/v1/admin/games/{id}/force-complete - Force complete round
```

## 🗄️ Database Schema

### Tables

#### `players`
```
id: string (UUID)
displayname: string (unique)
password_hash: string (Argon2id)
is_admin: bool
bio: option<string>
avatar_url: option<string>
created_at: int (unix timestamp)
elo_rating: int (default: 1200)
```

#### `sessions`
```
id: string (UUID)
player_id: string (UUID)
expires_at: int (unix timestamp)
```

#### `games`
```
id: string (UUID)
game_state: string (JSON-serialized Game)
lifecycle: string (Waiting|InProgress|Finished)
created_at: int
created_by: string (UUID)
player_count: int
```

#### `game_players`
```
game_id: string
player_id: string
player_pid: int (0, 1, 2, ...)
```

#### `game_events`
```
game_id: string
timestamp: int
event_kind: string (MOVE, PLAYER_JOINED, GAME_STARTED, etc.)
event_data: string (JSON)
```

#### `chat_messages`
```
game_id: string
timestamp: int
player_id: string
displayname: string
message: string
```

#### `snapshots`
```
game_id: string
index: int
timestamp: int
snapshot_data: string (JSON-serialized GameSnapshot)
```

#### `matchmaking_queue`
```
player_id: string (unique)
queued_at: int
game_preferences: string (JSON: {player_count, use_column_rule})
```

#### `player_stats`
```
player_id: string (unique)
games_played: int
games_won: int
total_playtime: int (seconds)
```

## ⚙️ Configuration

### Environment Variables

```bash
# Database
DATABASE_URL=rocksdb://automatafl.db          # Default: embedded RocksDB
# Or use external SurrealDB:
# DATABASE_URL=ws://localhost:8000

# Server
BIND_ADDRESS=127.0.0.1:3000                   # Default: 127.0.0.1:3000
SESSION_DURATION=86400                        # Default: 24 hours (in seconds)

# OpenTelemetry (optional)
OTEL_ENDPOINT=http://localhost:4317           # OTLP collector endpoint
OTEL_SERVICE_NAME=automatafl-backend          # Service name for traces/metrics

# Matchmaking
MATCHMAKING_INTERVAL=5                        # Seconds between matching attempts (default: 5)
MATCHMAKING_ELO_TOLERANCE=200                 # ELO difference for matching (default: 200)
```

### Database Options

#### Embedded RocksDB (Default)
```bash
# Data stored in ./automatafl.db/
cargo run
```

#### External SurrealDB
```bash
# Start SurrealDB
surreal start --bind 0.0.0.0:8000 file://data.db

# Set environment variable
export DATABASE_URL=ws://localhost:8000
cargo run
```

## 🔒 Security Features

### Password Security
- **Argon2id** hashing algorithm (winner of Password Hashing Competition)
- Per-user salts generated with cryptographically secure RNG
- Resistant to GPU/ASIC attacks

### Session Management
- UUID-based session tokens
- 24-hour expiration (configurable)
- Sessions stored in database for persistence across restarts
- Automatic cleanup of expired sessions

### API Security
- CORS configured (permissive by default, customize for production)
- Session validation on all protected endpoints
- Admin-only endpoints check `is_admin` flag
- Players can only modify their own profiles

### Input Validation
- Move validation through game logic
- Displayname uniqueness enforced at database level
- Type-safe deserialization via Serde

## 🛠️ Development

### Prerequisites
- Rust 1.80+ (uses `edition = "2024"`)
- SurrealDB (optional, for external database)

### Building
```bash
cargo build
```

### Running
```bash
cargo run
```

### Testing
```bash
cargo test
```

### Development with auto-reload
```bash
cargo install cargo-watch
cargo watch -x run
```

### Checking compilation
```bash
cargo check
```

## 📊 Observability

### Logging
Uses `tracing` crate for structured logging:
```rust
tracing::info!("Server started on {}", addr);
tracing::error!("Database error: {}", err);
```

### Metrics (OpenTelemetry)
Tracked metrics:
- Request latency (histogram)
- Active games (gauge)
- Active players (gauge)
- Games created/completed (counter)
- API errors by type (counter)
- Matchmaking queue size (gauge)

### Tracing
All endpoints instrumented with spans for distributed tracing.

## 🚢 Deployment

### Docker
```dockerfile
FROM rust:1.80 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/automatafl-backend /usr/local/bin/
EXPOSE 3000
CMD ["automatafl-backend"]
```

### Environment-based Configuration
```bash
# Production
export DATABASE_URL=ws://surrealdb:8000
export BIND_ADDRESS=0.0.0.0:3000
export OTEL_ENDPOINT=http://otel-collector:4317
./automatafl-backend
```

### Graceful Shutdown
The server handles SIGTERM/SIGINT for graceful shutdown:
- Completes in-flight requests
- Closes database connections
- Stops background tasks

## 🎮 Game Flow

### Creating a Game
1. Client: `POST /api/v1/games` with `{player_count: 2, use_column_rule: true}`
2. Server: Creates game in database, returns `game_id`
3. Server: Creates broadcast channel for real-time events

### Joining a Game
1. Client: `POST /api/v1/games/{id}` (authenticated)
2. Server: Adds player to `game_players`, assigns `player_pid`
3. Server: Broadcasts `PLAYER_JOINED` event
4. Server: Auto-starts game when `player_count` reached

### Real-time Updates
1. Client: Connect to `ws://server/api/v1/games/{id}/ws`
2. Server: Sends initial `STATE` event with full game state
3. Server: Sends heartbeat pings every 30 seconds
4. Server: Broadcasts events: `MOVE_ACK`, `MOVE`, `AUTOMATON_STEP`, `GAME_OVER`, etc.

### Making Moves
1. Client: `POST /api/v1/games/{id}/move` with `{from: {x, y}, to: {x, y}}`
2. Server: Validates move, stores in database
3. Server: Broadcasts `MOVE_ACK` or `MOVE_INVALID`
4. Server: Auto-completes round when all moves submitted
5. Server: Broadcasts results: `MOVE`, `AUTOMATON_STEP`, `ROUND_COMPLETE` or `CONFLICTS`

### Matchmaking Flow
1. Client: `POST /api/v1/matchmaking/join` with preferences
2. Server: Adds to queue in database
3. Background task (every 5s): Matches players with same preferences
4. Server: Creates game, adds players, removes from queue
5. Clients poll `/api/v1/matchmaking/status` or use WebSocket notifications

## 📝 API Examples

### Register & Login
```bash
# Register
curl -X POST http://localhost:3000/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{"displayname": "alice", "password": "secret123"}'

# Login
curl -X POST http://localhost:3000/api/v1/login \
  -H "Content-Type: application/json" \
  -d '{"displayname": "alice", "password": "secret123"}'
# Returns: {"session_id": "...", "player_id": "..."}
```

### Create & Join Game
```bash
# Create game
curl -X POST http://localhost:3000/api/v1/games \
  -H "Authorization: Bearer <session_id>" \
  -H "Content-Type: application/json" \
  -d '{"player_count": 2, "use_column_rule": true}'
# Returns: "<game_id>"

# Join game
curl -X POST http://localhost:3000/api/v1/games/<game_id> \
  -H "Authorization: Bearer <session_id>"
# Returns: player_pid (0 or 1)
```

### Submit Move
```bash
curl -X POST http://localhost:3000/api/v1/games/<game_id>/move \
  -H "Authorization: Bearer <session_id>" \
  -H "Content-Type: application/json" \
  -d '{"from": {"x": 5, "y": 5}, "to": {"x": 6, "y": 5}}'
```

### Get Game History (with filtering)
```bash
# All events
curl http://localhost:3000/api/v1/games/<game_id>/history \
  -H "Authorization: Bearer <session_id>"

# Only MOVE events since timestamp
curl "http://localhost:3000/api/v1/games/<game_id>/history?since=1234567890&event_kind=MOVE" \
  -H "Authorization: Bearer <session_id>"
```

### Matchmaking
```bash
# Join queue
curl -X POST http://localhost:3000/api/v1/matchmaking/join \
  -H "Authorization: Bearer <session_id>" \
  -H "Content-Type: application/json" \
  -d '{"player_count": 2, "use_column_rule": true}'

# Check status
curl http://localhost:3000/api/v1/matchmaking/status \
  -H "Authorization: Bearer <session_id>"
```

## 🔧 Troubleshooting

### Database Connection Issues
```bash
# Check if database is accessible
surreal sql --endpoint ws://localhost:8000 --ns automatafl --db main

# View database contents
SELECT * FROM players LIMIT 10;
SELECT * FROM games WHERE lifecycle = "InProgress";
```

### Session Expired Errors
- Sessions expire after 24 hours
- Re-login to get new session token
- Check `SESSION_DURATION` environment variable

### WebSocket Connection Drops
- Heartbeat pings sent every 30 seconds
- Check firewall/proxy settings for WebSocket support
- Verify game exists before connecting

### Matchmaking Not Finding Matches
- Ensure preferences (player_count, use_column_rule) match exactly
- Background task runs every 5 seconds
- Check queue: `SELECT * FROM matchmaking_queue;`

## 🏛️ Architecture Decisions

### Why SurrealDB?
- Embedded mode for easy deployment (no separate DB process)
- SQL-like queries familiar to developers
- Built-in indexing and relationships
- Can scale to external deployment

### Why Argon2 over bcrypt?
- Modern algorithm (2015 vs 1999)
- Better resistance to GPU attacks
- Configurable memory hardness
- Recommended by OWASP

### Why Session-based Auth?
- Simple to implement and understand
- Works well with WebSocket connections
- Session data stored in database (survives restarts)
- Easy to implement logout/revocation

### Why DashMap for Game Channels?
- Lock-free concurrent HashMap
- Per-game isolation (no contention)
- Real-time broadcast without database queries
- Automatic cleanup when games are removed

## 📚 Code Structure

```
backend/src/
├── main.rs           # Server setup, routes, endpoints
├── db.rs             # Database functions and schema
└── templates/        # Askama HTML templates (optional)

api-types/src/
└── lib.rs            # Shared request/response types

logic/src/
├── game.rs           # Core game logic
└── impls.rs          # Game implementations
```

## 🔮 Future Enhancements

- [ ] WebSocket notifications for matchmaking (client-initiated)
- [ ] Replay system using stored game events
- [ ] Tournament mode with brackets
- [ ] Spectator mode (view-only WebSocket)
- [ ] Rate limiting per player/IP
- [ ] OAuth integration (Discord, GitHub)
- [ ] Email verification
- [ ] Password reset flow
- [ ] Game invites (direct player-to-player)

## 📄 License

Same as parent project (Apache-2.0 OR MIT)
