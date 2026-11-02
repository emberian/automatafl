# Automatafl

Formally verified simultaneous-move tactical board game where two players manipulate **Attractors (`A`)** and **Repulsors (`R`)** along rook sightlines to herd a deterministic **Automaton (`@`)** into their corner goals under fog-of-war commit-reveal rules.

Live at **<https://automatafl.fg-goose.online>**.

---

## Architecture (`Zero-Twin` Lean 4 Core)

Every game rule, sightline projection, simultaneous-move conflict resolution, and automaton step exists in **one place only**: the formally verified **Lean 4** kernel under [`lean/Automatafl/`](lean/Automatafl/).

- **No Duplicate Rust Rule Engine**: Lean 4 compiles to C IR (`lean/.lake/build/ir/Automatafl/*.c`), which [`automatafl-lean`](automatafl-lean/) compiles and links directly into native binaries and WebAssembly targets via a stripped Lean runtime shim ([`lean_shim.c`](automatafl-lean/c/lean_shim.c)).
- **Client-Offloaded Computation**: Move legality, destination forecasting, sightline loop highlights, and optional client-side AI bot evaluation run locally on the client, keeping server CPU and memory usage minimal (~640 KB RSS in production).

```text
lean/Automatafl/
├── Base.lean            Core board, particle, sightline, and axial ray definitions + proofs
├── Rules.lean           Canonical simultaneous-move conflict resolution, automaton step & theorems
├── Termination.lean     Well-founded termination proofs for conflict-resolution re-entry
├── Variants.lean        Parameterized variants (tie-breaks, bunch/annihilate, 2-cycle swaps, cooldowns, railgun, multi-automata)
├── PackedBoard.lean     2-bit-per-cell UInt64/ByteArray packed board + Equivalence theorems to Board
├── FFI.lean             @[export] C/WASM entrypoints (`automatafl_lean_exec`, `automatafl_packed64_*`)
└── *Fixtures.lean       Compile-time `#guard` conformance suites

Workspace Crates:
├── automatafl-lean/     Rust FFI bridge & zero-allocation 64-bit packed board bindings to Lean 4 C IR
├── automatafl-ai/       Simultaneous-move AI bots (Novice, Tactical, Minimax, Decoupled UCT, Regret Matching) + WASM exports
├── automatafl-server/   Axum fog-of-war server, commit-reveal protocol, LRU + bounded disk persistence, SSE stream, embedded web UI
├── automatafl-client/   Multi-target client library & native terminal TUI (`automatafl-tui`)
└── v8n/                 Property-based (`proptest`), spec, merge-pathway, and win-condition test suites
```

---

## Game Rules & Variants

1. **Particles & Axial Rook Movement**:
   - **Repulsor (`R`, code `1`)** and **Attractor (`A`, code `2`)** are shared field pieces: either player may move any `R` or `A` (except squares marked by a same-turn conflict or on cooldown).
   - Pieces slide orthogonally like a chess rook through vacuum (`0`) and may not jump over other particles.
2. **Simultaneous Commit & Conflict Resolution**:
   - Both seats (`A` and `B`) seal their moves simultaneously.
   - **Conflict Mode (default)**: If both players target the same destination square (or pick up the same source piece), that square is **marked (`X`)** for the remainder of the turn and colliding seats re-submit until their moves are disjoint.
   - **Bunch Before Merge**: Colliding movers slide along their rays and stop one cell short of the contested square (`MERGE_RESOLUTION_DESIGN.md`).
   - **Annihilate**: Movers converging on the same destination destroy each other.
3. **The Automaton (`@`, code `3`)**:
   - After player moves resolve, the Automaton scans all four orthogonal sightlines (`left`, `right`, `up`, `down`) for the nearest particle:
     - **Repulsor (`R`)** pushes `@` **1 square away** (requires vacuum behind `@`).
     - **Attractor (`A`)** pulls `@` **1 square toward itself** (requires distance $\ge 2$ so the adjacent square is vacuum).
   - Force priority is governed by the active **Tie-Break Rule**:
     - `column`: Column forces (`up`/`down`) override row forces (`left`/`right`); opposite equal-axis forces cancel.
     - `row`: Row forces (`left`/`right`) override column forces (`up`/`down`).
     - `nearest_strict`: Strictly nearest active force wins; equal-distance ties cancel.
     - `nearest_then_column` / `nearest_then_row`: Nearest active force wins, breaking equal-distance ties by axis.
     - `pure_cancel`: Any two distinct active force directions cancel out.

See [`logic/PHILOSOPHY.md`](logic/PHILOSOPHY.md), [`logic/MOVE_EXPLAIN.md`](logic/MOVE_EXPLAIN.md), [`MERGE_RESOLUTION_DESIGN.md`](MERGE_RESOLUTION_DESIGN.md), and [`V8N_OVERVIEW.md`](V8N_OVERVIEW.md) for design background.

---

## Building & Running

### 1. Verify the Lean 4 Core (and regenerate C IR if modified)
```bash
cd lean
lake build
```
*(Pre-generated Lean 4 C IR is checked into `lean/.lake/build/ir/Automatafl/*.c`, so `cargo build` and `cargo test` work without a local Lean toolchain.)*

### 2. Run the Web & Fog-of-War Server
```bash
cargo run --release -p automatafl-server --bin automatafl-server -- --bind 127.0.0.1:3030 --data-dir ./data
```
Then open <http://127.0.0.1:3030>.

### 3. Run the Native Terminal UI (`automatafl-tui`)
```bash
# Local offline game vs Tactical Bot
cargo run --release -p automatafl-client --bin automatafl-tui -- --preset mini5x5 --bot tactical

# Or connect to a remote server
cargo run --release -p automatafl-client --bin automatafl-tui -- --server https://automatafl.fg-goose.online
```

### 4. Run the Verification & Property-Test Suites
```bash
cargo test --workspace
```
