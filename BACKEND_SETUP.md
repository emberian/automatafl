# Automatafl Backend Setup Guide

## Overview

I've created a comprehensive Axum-based backend for the Automatafl game, similar to lichess but for your board game. The backend includes all the requested features:

- ✅ User authentication (JWT-based)
- ✅ New user registration
- ✅ Lobby/game browser
- ✅ Ability to spectate games
- ✅ In-game chat
- ✅ Game history viewing
- ✅ RESTful API endpoints (no websockets as requested)

## Architecture

### Project Structure
```
backend/
├── src/
│   ├── main.rs          # Server entry point
│   ├── auth.rs          # JWT authentication middleware
│   ├── db.rs            # Database utilities
│   ├── error.rs         # Error handling
│   ├── models.rs        # Data models
│   ├── state.rs         # Application state
│   └── handlers/        # Request handlers
│       ├── mod.rs
│       ├── auth.rs      # Authentication endpoints
│       ├── chat.rs      # Chat endpoints
│       ├── game.rs      # Game management endpoints
│       └── history.rs   # Game history endpoints
├── migrations/          # Database migrations
├── examples/
│   └── client.rs        # Example API client
├── build.rs             # Build script
└── README.md            # Backend documentation
```

### Key Features

1. **Authentication System**
   - JWT-based authentication
   - User registration with password hashing (bcrypt)
   - ELO rating system (starts at 1200)

2. **Game Management**
   - Create new games
   - Join waiting games
   - Submit moves with validation
   - Automatic conflict resolution
   - Spectator mode

3. **Real-time Features**
   - In-game chat for players and spectators
   - Game state updates after each move
   - Active games stored in memory (DashMap)

4. **Game History**
   - View complete game history with all moves
   - User game history with pagination
   - Game replay functionality

## Setup Instructions

1. **Environment Variables**
   Create a `.env` file in the backend directory:
   ```
   DATABASE_URL=sqlite:automatafl.db
   JWT_SECRET=your-secret-key-change-in-production
   RUST_LOG=info
   ```

2. **Install SQLx CLI** (optional, for migrations):
   ```bash
   cargo install sqlx-cli --no-default-features --features sqlite
   ```

3. **Run Migrations**:
   ```bash
   cd backend
   sqlx migrate run
   ```

4. **Run the Server**:
   ```bash
   cargo run
   ```

The server will start on `http://localhost:3000`

## API Documentation

### Authentication Endpoints

#### Register
```
POST /api/auth/register
Body: { "username": "string", "password": "string" }
Response: { "token": "jwt_token", "user": {...} }
```

#### Login
```
POST /api/auth/login
Body: { "username": "string", "password": "string" }
Response: { "token": "jwt_token", "user": {...} }
```

### Game Endpoints

#### List Games
```
GET /api/games
Response: Array of active/waiting games
```

#### Create Game
```
POST /api/games/create
Headers: Authorization: Bearer <token>
Response: Game state object
```

#### Join Game
```
POST /api/games/:game_id/join
Headers: Authorization: Bearer <token>
Response: Updated game state
```

#### Submit Move
```
POST /api/games/:game_id/move
Headers: Authorization: Bearer <token>
Body: { "from_x": 0, "from_y": 0, "to_x": 1, "to_y": 0 }
Response: { "feedback": "string", "game_state": {...} }
```

### Chat Endpoints

#### Get Messages
```
GET /api/games/:game_id/chat?limit=50&before=<timestamp>
Response: Array of chat messages
```

#### Send Message
```
POST /api/games/:game_id/chat
Headers: Authorization: Bearer <token>
Body: { "message": "string" }
Response: Created message object
```

## Database Schema

- **users**: User accounts with ratings
- **games**: Game states and metadata
- **game_moves**: Move history for each game
- **chat_messages**: In-game chat messages
- **spectators**: Users watching games

## Next Steps

1. **Frontend Development**: Build a web interface using the webapp crate
2. **WebSocket Support**: Add real-time updates (currently uses polling)
3. **Matchmaking**: Add automatic pairing based on ratings
4. **Tournaments**: Support for tournament play
5. **Analysis**: Post-game analysis features

## Testing

Run the example client to test the API:
```bash
# Add to backend/Cargo.toml:
# [dev-dependencies]
# reqwest = { version = "0.11", features = ["json"] }

cargo run --example client
```

The backend is fully functional and ready for frontend integration!
