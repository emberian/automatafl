# automatafl-v8n: Validation & Testing Infrastructure

This crate provides comprehensive validation, testing, and AI infrastructure for automatafl.

## Overview

The `v8n` crate has been extracted from `logic` to keep the core game logic lean while providing
extensive testing and validation capabilities. It includes:

- **Composable move generation** - Flexible, combinable strategies for exhaustive testing
- **Board builders** - Create test boards with specific characteristics
- **Game recording/replay** - Capture and validate game sessions
- **Property-based tests** - Verify game logic across arbitrary inputs
- **AI players** - Minimax, MCTS, and AlphaZero-lite (stubs for implementation)
- **Comprehensive benchmarks** - Performance testing from micro to macro scale

## Architecture

```
v8n/
├── src/
│   ├── lib.rs                  # Main module
│   ├── board_builders.rs       # Board creation utilities
│   ├── recording.rs            # Game recording and replay
│   └── move_gen/               # Composable move generation
│       ├── mod.rs              # Core MoveGenerator trait
│       ├── basic.rs            # Basic generators (Random, ConflictSeeking, etc.)
│       ├── combinators.rs      # Composable combinators
│       └── ai/                 # AI players
│           ├── minimax.rs      # Minimax with alpha-beta pruning
│           ├── mcts.rs         # Monte Carlo Tree Search
│           └── alphazero.rs    # AlphaZero-lite
├── tests/
│   ├── handwritten_spec_tests.rs  # Handwritten integration tests
│   └── property_tests.rs          # Property-based tests with proptest
├── benches/
│   └── benches.rs              # Comprehensive benchmarks
└── examples/
    └── composable_generators.rs   # Usage examples

## Composable Move Generation

The move generation system is designed around composition and flexibility.

### Core Trait

```rust
pub trait MoveGenerator {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]>;
}
```

### Basic Generators

- **RandomMoveGen** - Generates random valid moves for all players
- **ConflictSeekingGen** - Intentionally creates source or destination conflicts
- **ChainMakingGen** - Creates chains and cycles of piece movements
- **GoalOrientedGen** - Strategically moves pieces toward player goals

### Combinators

Combinators allow generators to be composed in sophisticated ways:

- **SequentialGen** - Try generators in sequence until requirements are met
- **MergedGen** - Combine moves from multiple generators
- **FilteredGen** - Filter moves based on a predicate
- **TransformedGen** - Transform moves from a generator
- **WeightedGen** - Randomly select from generators with weights
- **AdaptiveGen** - Choose generator based on game state analysis
- **RoundRobinGen** - Cycle through generators deterministically

### Example Usage

```rust
use automatafl_v8n::*;

// Create a weighted composite generator
let mut generator = WeightedGen::with_seed(
    vec![
        (7, Box::new(RandomMoveGen::new(42))),      // 70% random
        (2, Box::new(ConflictSeekingGen::new(43))), // 20% conflict
        (1, Box::new(GoalOrientedGen::new(44))),    // 10% goal-oriented
    ],
    seed,
);

// Use it in a game simulation
let moves = generator.generate_moves(&game);
```

### Advanced Composition

Combinators can be nested for complex behaviors:

```rust
// Filtered weighted generator - only even players, weighted strategies
let mut complex = FilteredGen::new(
    Box::new(WeightedGen::with_seed(
        vec![
            (5, Box::new(RandomMoveGen::new(42))),
            (3, Box::new(ConflictSeekingGen::new(43))),
            (2, Box::new(GoalOrientedGen::new(44))),
        ],
        seed,
    )),
    |_game, m| m.who.0 % 2 == 0,  // Only even-numbered players
);
```

## Property-Based Testing

Property tests verify game logic holds for arbitrary valid inputs using proptest.

### Test Categories

1. **Coordinate Properties**
   - Index round-tripping
   - Subtraction produces correct deltas
   - Delta arithmetic identity

2. **Delta Properties**
   - Zero detection
   - Axial detection
   - Scaling preserves direction
   - Displacement equals Manhattan distance

3. **Move Validation**
   - Axial moves accepted
   - Diagonal moves rejected
   - Out-of-bounds rejected

4. **Game State Invariants**
   - Board size preservation
   - Automaton always on board
   - Fresh initial state
   - No locked players at start
   - No pending moves at start

5. **Generator Properties**
   - Valid player IDs
   - Respects player count
   - Sequential fallback behavior

6. **Recording Properties**
   - Empty recordings validate
   - Round count consistency

Run property tests with:
```bash
cargo test --test property_tests
```

## Board Builders

Create boards with specific characteristics for testing:

```rust
// Large board with controlled density
let board = create_large_board(
    width: 127,
    height: 127,
    num_players: 32,
    density: 0.3,  // 30% filled
    seed: 42,
);

// Symmetric board with balanced positions
let board = create_symmetric_board(
    size: 64,
    num_players: 8,
    seed: 42,
);

// Stress test scenarios
let board = create_stress_test_board(
    StressTestScenario::MaxChain,
    size: 31,
);
```

## Game Recording

Record and replay game sessions for validation:

```rust
// Create recording
let mut recording = GameRecording::new(
    "Test game".to_string(),
    initial_board,
    player_count,
    use_column_rule,
);

// Add rounds
recording.add_round(moves, result);

// Set outcome
recording.set_outcome(GameOutcome::Winner(Pid(0)));

// Save to file (binary format)
recording.save_postcard("game.bin")?;

// Load and replay
let recording = GameRecording::load_postcard("game.bin")?;
let final_game = recording.replay()?;
```

## Benchmarks

Comprehensive benchmarks covering:

- **Micro benchmarks** - Individual operations (coord ops, delta ops, etc.)
- **Macro benchmarks** - Integrated workflows (game creation, move proposal, etc.)
- **Large-scale benchmarks** - Stress testing (127x127 boards, 32 players)

Run benchmarks with:
```bash
cargo bench
```

## AI Players (Planned)

Three AI approaches with varying sophistication:

### Minimax
- Classic adversarial search with alpha-beta pruning
- Heuristic evaluation functions
- Depth-limited search
- Best for deterministic analysis

### MCTS (Monte Carlo Tree Search)
- Probabilistic evaluation through simulation
- UCT (Upper Confidence Bound for Trees) for exploration/exploitation
- Better for high branching factors and simultaneous moves

### AlphaZero-lite
- Neural network + MCTS hybrid
- Network provides value estimates and move priors
- Self-play training capability (optional)
- Simplified version suitable for smaller boards

## Development Workflow

1. **Run all tests**
   ```bash
   cargo test
   ```

2. **Run property tests**
   ```bash
   cargo test --test property_tests
   ```

3. **Run benchmarks**
   ```bash
   cargo bench
   ```

4. **Run examples**
   ```bash
   cargo run --example composable_generators
   ```

## Future Directions

- Complete minimax implementation
- Complete MCTS implementation
- Complete AlphaZero-lite implementation
- Add more property-based tests
- Tournament system for AI vs AI
- Automated game tree analysis
- Training infrastructure for AlphaZero

## Dependencies

Core:
- `automatafl-logic` - Game logic
- `rand` - Random number generation
- `serde` - Serialization
- `smallvec` - Zero-allocation gameplay
- `ndarray` - Board storage

Testing:
- `proptest` - Property-based testing
- `criterion` - Benchmarking
- `rstest` - Parameterized tests

Optional:
- `postcard` - Binary serialization for recordings
- `ron` - Human-readable serialization (tests only)
