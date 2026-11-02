# Merge Resolution Design

**See also**: `logic/PHILOSOPHY.md` for the design principles underlying these decisions.

## Problem Statement

When multiple move chains converge on the same destination through vacuum squares:
```
A → X → M
B → Y → M
```

The current conflict rules don't detect this because X and Y are vacuum. Both pieces want to end at M.

This violates the **Principle of Fairness**: there's no intrinsic ordering between A and B that determines who gets M. It also tests the **Weakest Precondition** boundary: is this ambiguous enough to require a conflict?

## Solution: Two-Phase Approach

### Phase 1: Conflict Detection (Weakest Precondition)
Strengthen `resolve_conflicts` to optionally detect merging pathways based on game mode.

### Phase 2: Apply Moves (Resolution Strategy)
Handle allowed merges with configurable strategies that respect the Principle of Fairness.

## Merge Resolution Modes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MergeResolutionMode {
    /// Detect merging pathways in conflict phase and reject them
    /// Extends the weakest precondition to treat merges as conflicts
    DetectAndConflict,

    /// Allow merges, but annihilate all pieces that converge
    /// (Useful for tactical "denial" plays: "If I can't have it, nobody can")
    Annihilate,

    /// Pieces stop one square BEFORE the merge point if it would cause collision
    /// Merge point M remains empty when multiple chains converge on it
    BunchBeforeMerge,

    /// Pieces "stack up" along chains, stopping when they hit another piece
    /// (@Grissess): Process in reverse path order - each piece moves as far as it can
    /// First piece along longest path gets M, others compress behind
    BunchedStacking,
}
```

## Cycle Behavior Modes

**Important**: These modes only apply to cycles of length **>2**:
- **2-cycles** (e.g., A→B, B→A) always stay in place (unambiguous: {A→A} composition)
- **1-cycles** (e.g., A→A) are forbidden by `propose_move` (from==to check)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CycleBehaviorMode {
    /// Pieces advance one position around cycles of length >2
    /// (Original semantics from 2-player game)
    RotatePieces,

    /// All moves succeed but pieces remain in place even for >2-cycles
    /// (@Grissess): "Every edge fired once" but no material movement
    /// Useful for maintaining board state during automaton step
    NoMovement,
}
```

## Implementation Plan

### 1. Enhanced Conflict Detection

Add `detect_merging_pathways()` that:
- Builds the full move graph (like apply_moves does)
- Uses Tarjan SCC to find structure
- Traces each piece to its final destination
- If multiple pieces with non-vacuum sources converge → **CONFLICT**

This is called conditionally in `resolve_conflicts` based on mode.

### 2. Resolution Strategies

#### DetectAndConflict
- Never reaches apply_moves with merges
- Clean and predictable
- Extends weakest precondition: treats ambiguous merges as conflicts

#### Annihilate
- Current implementation (Phase 5 of apply_moves)
- Multiple pieces converging → all destroyed
- Tactical: "If I can't have it, nobody can"
- Simple but harsh

#### BunchBeforeMerge
When multiple chains A→X→M and B→Y→M converge:
1. Detect that M is a merge point (multiple paths lead to it)
2. Pieces stop one square BEFORE M (at X and Y respectively)
3. M remains **empty** (no piece occupies it)
4. Creates "traffic jam" where merge point is blocked

**Example**: A at (0,0), B at (2,0), both trying to reach M at (1,0) via vacuum squares:
- Result: Pieces bunch at X and Y, M stays empty

#### BunchedStacking (@Grissess)
Algorithm for general chains (including merges):
1. Build move graph and identify all pieces at source squares
2. For each chain X1 → X2 → ... → Xn, process in **reverse path order**:
   - X(n-1) tries to move to Xn
   - X(n-2) tries to move to X(n-1) (may be blocked if X(n-1) couldn't move)
   - Continue backward...
3. Each piece moves as far along its path as possible until:
   - It reaches the end of the chain, OR
   - The next square is occupied by another piece
4. Result: m pieces compress into positions X(n-m) through Xn

**For merging pathways**: The first piece to "claim" M (by path length or other topology metric) occupies it; subsequent pieces bunch behind.

**Key insight**: Order of processing emerges from **graph topology** (reverse path order, path length), not player IDs or submission time. This respects the Principle of Fairness.

#### Cycle Handling

**Fixed behavior** (not configurable):
- **2-cycles** (A→B, B→A): Always stay in place - unambiguous composition {A→A}
- **1-cycles** (A→A): Forbidden by `propose_move` validation (from==to check)

**Configurable for >2-cycles**:

**RotatePieces**:
- In populated cycles of length >2, each piece advances one position
- Honors "every edge fired once" with material movement
- Original semantics from 2-player game

**NoMovement** (@Grissess):
- All moves in >2-cycle succeed but pieces remain in place
- "Every edge fired once" but no material movement
- Allows tactical "hold position" during automaton step
- Extends 2-cycle semantics to longer cycles

## Weakest Precondition Analysis

### What MUST be conflicts?
1. Multiple pieces from non-vacuum sources to same immediate destination
2. Same source with multiple destinations (fork)
3. **[Optional]** Multiple pieces converging after chain resolution

### What CAN be handled creatively?
1. Pieces passing through shared vacuum waypoints (different times)
2. Cycles with careful rotation
3. Complex fork/join topologies if pieces don't actually collide

## Configuration

Add to `Game`:
```rust
pub merge_resolution_mode: MergeResolutionMode,
pub cycle_behavior_mode: CycleBehaviorMode,
```

Update `Game::new()` to accept these parameters.

**Recommended defaults**:
- `merge_resolution_mode: MergeResolutionMode::BunchedStacking` (most interesting gameplay)
- `cycle_behavior_mode: CycleBehaviorMode::RotatePieces` (preserves original 2-player semantics)

## Testing Strategy

Create test suites for each mode:

**Merge resolution tests**:
- `merging_pathways_detect_mode`: Should create conflicts
- `merging_pathways_annihilate_mode`: Pieces disappear (current behavior)
- `merging_pathways_bunched_stacking_mode`: Traffic jams form, pieces compress at end

**Cycle behavior tests**:
- `cycle_rotation_mode`: Pieces advance one position
- `cycle_no_movement_mode`: Pieces stay in place, moves succeed

## Migration Path

### Phase 1: Type Definitions
1. Add `MergeResolutionMode` enum to `logic/src/types.rs`
2. Add `CycleBehaviorMode` enum to `logic/src/types.rs`
3. Add fields to `Game` struct
4. Update `Game::new()` signature

### Phase 2: BunchedStacking Implementation
1. Keep current Phase 5 annihilation logic as fallback
2. Add new Phase 4.5: "Reverse Path Compression"
   - For non-cycle chains, simulate pieces moving in reverse order
   - Track "would collide" conditions
   - Update destination_pieces accordingly
3. Make Phase 5 conditional on `merge_resolution_mode`

### Phase 3: Cycle Behavior Configuration
1. Make Phase 3 (cycle handling) conditional on `cycle_behavior_mode`
2. For `NoMovement`: still populate `resolved_piece_coords` but set destinations = sources

### Phase 4: Enhanced Conflict Detection (Optional)
1. Implement `detect_merging_pathways()` helper
2. Call from `resolve_conflicts()` when mode is `DetectAndConflict`
3. Mark merge points as conflicted

### Phase 5: Testing and Documentation
1. Update existing tests with explicit mode configuration
2. Add mode-specific tests
3. Update README.md with new gameplay options

## Open Questions

1. Should 1-cycles (self-loops A→A) be treated specially? Currently they rotate (no-op).
2. **Merging pathways in BunchedStacking**: Do pieces bunch **before** the merge point M, or does the first-to-arrive get M?
   - Option A: M becomes occupied by first piece along longest path, others bunch before M
   - Option B: All pieces bunch one square before M (M remains empty)
3. Do we want hybrid modes? (e.g., "Bunch with 2-piece limit, then annihilate")
4. Should DetectAndConflict mode mark just the merge point M, or all squares in both chains?
