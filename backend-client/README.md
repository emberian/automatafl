# Automatafl Backend Client

A Rust client library and CLI tool for interacting with the Automatafl backend API.

## Features

- 🦀 **Cross-platform**: Works on both native (via Tokio) and WASM (via wasm-bindgen)
- 📚 **Library**: Use as a dependency for web frontends or other Rust projects
- 🖥️ **CLI Tool**: Interactive command-line interface for testing and exploration
- 🔐 **Session Management**: Automatic session token storage and reuse
- 🎨 **Colored Output**: Beautiful terminal output with helpful formatting

## Installation

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
automatafl-backend-client = { path = "../backend-client" }
```

### As a CLI Tool

Build and install:

```bash
cargo install --path . --features cli
```

Or run directly:

```bash
cargo run --features cli -- --help
```

## CLI Usage

### Health Check

```bash
automatafl-client health
```

### Authentication

Register a new player:
```bash
automatafl-client register alice mypassword
```

Login:
```bash
automatafl-client login alice mypassword
```

Logout:
```bash
automatafl-client logout
```

### Game Management

List all games:
```bash
automatafl-client game list
```

Create a 2-player game:
```bash
automatafl-client game create --players 2
```

Create a 4-player game with column rule:
```bash
automatafl-client game create --players 4 --column-rule
```

Join a game:
```bash
automatafl-client game join <GAME_ID>
```

Get game state:
```bash
automatafl-client game state <GAME_ID>
```

Get goals:
```bash
automatafl-client game goals <GAME_ID>
```

### Moves

Check pending move:
```bash
automatafl-client move pending <GAME_ID>
```

Perform a move:
```bash
automatafl-client move do <GAME_ID> <FROM_X> <FROM_Y> <TO_X> <TO_Y>
```

Example - move from (0,1) to (0,2):
```bash
automatafl-client move do $GAME_ID 0 1 0 2
```

Manually complete round (if auto-completion is disabled):
```bash
automatafl-client move complete <GAME_ID>
```

### Chat

Send a message:
```bash
automatafl-client chat send <GAME_ID> Hello everyone!
```

View chat history:
```bash
automatafl-client chat history <GAME_ID>
```

### Advanced Options

Use a different backend URL:
```bash
automatafl-client --url http://example.com:3000 health
```

Use a specific session token:
```bash
automatafl-client --token <SESSION_TOKEN> game list
```

## Library Usage

```rust
use automatafl_backend_client::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client
    let mut client = AutomataflClient::new("http://localhost:3000");

    // Login
    let login = client.login("alice".to_string(), "password".to_string()).await?;
    println!("Session: {}", login.session_id);

    // Create a game
    let game_id = client.create_game(2, true).await?;
    println!("Created game: {}", game_id);

    // Join the game
    let pid = client.join_game(game_id).await?;
    println!("You are player: {}", pid.0);

    // Make a move
    let result = client.perform_move(
        game_id,
        Coord { x: 0, y: 1 },
        Coord { x: 0, y: 2 },
    ).await?;

    match result.feedback {
        MoveFeedback::Committed => println!("Move committed!"),
        _ => println!("Move failed: {:?}", result.feedback),
    }

    Ok(())
}
```

## Session Storage

The CLI automatically stores your session token in:
- **Linux**: `~/.config/automatafl-client/session.txt`
- **macOS**: `~/Library/Application Support/automatafl-client/session.txt`
- **Windows**: `%APPDATA%\automatafl-client\session.txt`

This means you don't need to login every time you use the CLI!

## WASM Support

To use in a web frontend:

```toml
[dependencies]
automatafl-backend-client = { path = "../backend-client" }
wasm-bindgen-futures = "0.4"
```

The client works seamlessly in both native and WASM environments.

## API Coverage

The client supports all backend endpoints:

- ✅ Health check
- ✅ Registration & authentication
- ✅ Game creation, listing, joining
- ✅ Move submission & completion
- ✅ Chat messaging
- ✅ Game state queries
- ✅ Save/load snapshots
- ✅ Admin endpoints (when authenticated as admin)

## Development

Build the library:
```bash
cargo build --manifest-path backend-client/Cargo.toml
```

Build the CLI:
```bash
cargo build --manifest-path backend-client/Cargo.toml --features cli
```

Run tests:
```bash
cargo test --manifest-path backend-client/Cargo.toml
```

## License

Apache-2.0 OR MIT
