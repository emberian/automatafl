Build a succinct, high-performance Lichess-tier platform for Automatafl across native and web environments, powered by a verified Lean 4 core compiled directly to C/WASM (zero-twin architecture), simultaneous-move AI bots, and a fog-of-war server.

Working directory: /Users/ember/dev/automatafl
Integrity mode: development

## Reference Material
- Source Lean specifications in `/Users/ember/dev/breadstuffs`:
  - `metatheory/Dregg2/Games/AutomataflRules.lean` (2,962 lines, 0 sorry: proves `resolve_conserves`, `resolve_perm`, `clashCoords_perm`, `winOnEntry_sound`, `adjudicate_seated`, `adjudicate_sound`, `adjudicate_draw_level`, `PairCollapse` closed forms)
  - `metatheory/Dregg2/Games/AutomataflTermination.lean` (585 lines: proves `roundStep_marks_strict_growth`, `RoundWF` preservation, `againDepth_le`, `runTurn_fuel_bound`)
  - `metatheory/Dregg2/Games/AutomataflFFI.lean` (581 lines: wire protocol `@[export dregg_automatafl_rules] def rulesFFI (s : String) : String`)
  - `docs/reference/AUTOMATAFL-RULES-CONFORMANCE-AUDIT.md`, `AUTOMATAFL-DIFFERENTIAL-FINDINGS.md`, `AUTOMATAFL-MIGRATION-DESIGN.md`, `AUTOMATAFL-WIRING-AND-SHIP-DESIGN.md`
  - `dregg-automatafl/src/rules.rs`
- Local assets & gate tests in `/Users/ember/dev/automatafl`:
  - `MERGE_RESOLUTION_DESIGN.md`
  - `logic/MOVE_EXPLAIN.md`
  - `v8n/tests/release_profile_gates.rs`
  - `v8n/tests/win_condition.rs`

## Requirements

### R1. Standalone Lean 4 Core & Direct C/WASM Compilation (Zero-Twin Architecture)
Extract the standalone, Mathlib-free Lean 4 package (`lean/`) into `/Users/ember/dev/automatafl` based on the proven canonical specification in `/Users/ember/dev/breadstuffs`, maintaining 0 sorry. Implement a two-tier ABI in Lean:
1. The string wire protocol (`rulesFFI`) for human play and UI queries (`stock`, `goals`, `sense`, `step`, `mid`, `turn`, `adjudicate`, `dist`, `legal`, `targets`, `livetargets`, `clash`, `round`).
2. A flat, unboxed `PackedBoard` representation (unboxed `UInt64` bitmasks / flat array) with exported C functions and a formally verified equivalence theorem (`unpack (PackedBoard.step b ms) = roundStep (unpack b) ms`) for allocation-free high-speed simulation.
Expose the compiled C/WASM core via a thin Rust FFI crate (`automatafl-lean`) targeting native (Server, Desktop, Mobile, TUI) and `wasm32`, with no independent unverified Rust rules twin.

### R2. GameConfig Generalizations & Variants in Lean
Support configurable game rules in Lean, including:
- Tie-break modes: `column`, `row`, `freeze`, `momentum (preferSameAxis : Bool)`
- Confluence modes: `conflict` (canonical mark & re-enter), `bunchBefore` (stop 1 cell short of collision without re-entry, per `MERGE_RESOLUTION_DESIGN.md`), `annihilate` (colliding movers destroy each other)
- Configuration fields: `twoCycleSwaps`, `cooldownTurns`, `railgunFling`, `automataCount`, and board presets (7×7 Blitz, 9×9 Tactical, 11×11 Classic).

### R3. Repository Cleanup & Asset Preservation
Preserve all valuable uncommitted work in `/Users/ember/dev/automatafl` (`MERGE_RESOLUTION_DESIGN.md`, `logic/MOVE_EXPLAIN.md`, `v8n/tests/release_profile_gates.rs`, `v8n/tests/win_condition.rs`). Remove or replace obsolete, broken Rust twins and stubs (`fastlogic/`, empty `ai/` stubs, obsolete `rust/` prototype, and broken experiment branches with move-leakage or duplicate-move bugs).

### R4. Simultaneous-Move AI Engine
Implement simultaneous-move computer player heuristics and search engines (Decoupled UCT / minimax / Regret Matching) operating directly over `PackedBoard` via native and WASM interfaces without leaking secret moves or duplicating moves. Game-theoretic formal proofs (such as first-player advantage analysis) are exploratory.

### R5. Platform Server & Matchmaking
Implement a fog-of-war simultaneous-move commit-phase server, matchmaking, and game persistence for public hosting.

### R6. Multi-Target Client User Interface
Implement a succinct, responsive, and efficient client user interface for Automatafl across Web, Desktop, Mobile, and/or TUI, using an efficient UI stack selected by the team.

## Acceptance Criteria

### Lean Verification & Compilation
- [ ] `lean/` compiles with Lean 4 without Mathlib or external dependencies with 0 errors and 0 sorry.
- [ ] Formal proofs for `resolve_conserves`, `resolve_perm`, `clashCoords_perm`, `winOnEntry_sound`, `adjudicate_seated`, `adjudicate_sound`, `adjudicate_draw_level`, termination bounds, and `PackedBoard` equivalence theorem are intact and pass verification.
- [ ] Lean core emits clean C code that builds for native targets and `wasm32`.
- [ ] `automatafl-lean` Rust FFI crate builds cleanly on native and `wasm32` and passes all conformance tests against the reference audit and `v8n/tests/release_profile_gates.rs`.

### Rules & Variants
- [ ] All `GameConfig` variants (tie-break modes, confluence modes including `bunchBefore`, board size presets, cooldown turns, railgun fling) are supported and executable through the FFI.
- [ ] Preserved uncommitted assets (`MERGE_RESOLUTION_DESIGN.md`, `logic/MOVE_EXPLAIN.md`, `v8n/tests/`) remain intact.

### AI Search & Engine Performance
- [ ] `PackedBoard` simulation executes allocation-free playouts at high throughput suitable for search.
- [ ] Simultaneous-move AI search bots (Decoupled UCT / minimax / Regret Matching) play valid games without leaking hidden moves or executing duplicate-player moves.

### Server & Client Platform
- [ ] Fog-of-war server enforces simultaneous-move commit phases, secret move protection, and game lifecycle/persistence.
- [ ] Client interface runs, renders game boards and fog-of-war state, and allows playing against AI bots or remote opponents.
