# Automatafl Backend

A REST API backend for the Automatafl game, built with Axum, similar to lichess but for automatafl!

## Features

- User authentication (JWT-based)
- Game creation and management
- Real-time game state updates
- Move submission with validation
- In-game chat
- Game history and replay
- Spectator mode
- Game browser/lobby

## Setup

1. Create a `.env` file with the following variables:
   ```
   DATABASE_URL=sqlite:automatafl.db
   JWT_SECRET=your-secret-key-change-in-production
   RUST_LOG=info
   ```

2. Run migrations:
   ```bash
   sqlx migrate run
   ```

3. Run the server:
   ```bash
   cargo run
   ```

The server will start on `http://localhost:3000`

## API Endpoints

### Authentication
- `POST /api/auth/register` - Register a new user
- `POST /api/auth/login` - Login and receive JWT token
- `GET /api/auth/me` - Get current user info (requires auth)

### Games
- `GET /api/games` - List active and waiting games
- `POST /api/games/create` - Create a new game
- `GET /api/games/:game_id` - Get game details
- `POST /api/games/:game_id/join` - Join a waiting game
- `POST /api/games/:game_id/move` - Submit a move
- `GET /api/games/:game_id/spectate` - Join as spectator

### Chat
- `GET /api/games/:game_id/chat` - Get chat messages
- `POST /api/games/:game_id/chat` - Send a chat message

### History
- `GET /api/games/:game_id/history` - Get full game history with moves
- `GET /api/users/:user_id/games` - Get user's game history

## Authentication

All authenticated endpoints require a JWT token in the Authorization header:
```
Authorization: Bearer <token>
```

## Game Rules

The game uses the automatafl-logic crate which implements the core game mechanics:
- 2 players (white and black)
- Board with particles (Attractors, Repulsors, Automaton)
- Players submit moves to manipulate particles
- The automaton moves automatically based on nearby particles
- Goal: Guide the automaton to your goal location

## Development

The backend uses:
- Axum for the web framework
- SQLx for database operations (SQLite)
- JWT for authentication
- bcrypt for password hashing
- DashMap for in-memory active game storage
