# Automatafl Design Philosophy

## Core Principles

### Principle of Largest Effect
**"Where possible, as many moves as can should succeed during a turn"**

The game mechanics should maximize the number of successful moves per turn, subject to physical constraints (occlusion, conflicts) and game rules. This principle drives many seemingly complex behaviors:

- Pieces can chain through vacuum squares to reach distant destinations
- Cycles allow pieces to rotate or remain in place (both count as "successful")
- Optimistic moves (moving from an expected-to-be-vacated square) enable team plays

### Principle of Fairness
**"The success and order of moves should be independent of any ordering on players or intrinsic ordering on the moves"**

Move resolution must not depend on:
- Player IDs or turn order
- Move submission time or sequence
- Any external ordering not inherent to the move graph topology

This principle rules out "first-come-first-served" or "player 0 goes first" tie-breaking. Any ordering used for resolution must emerge from the **structure of the move graph itself** (e.g., path lengths, graph topology, SCC structure).

### Weakest Precondition for Conflicts
**"Conflict rules should be the minimal checks that guarantee deterministic resolution"**

Conflicts occur only when the game state would become ambiguous or non-deterministic:

1. **Fork conflicts**: One source, multiple destinations (which way does the piece go?)
2. **Collision conflicts**: Multiple non-vacuum sources to one destination (which piece arrives?)
3. **[Optional, mode-dependent]** Merging pathway conflicts: Multiple chains converging through vacuum squares

The conflict detection phase should catch only what *cannot* be resolved deterministically during move application. Everything else can be handled with creative resolution strategies.

## Design Tensions and Solutions

### Merging Pathways (n>2 players)

When multiple move chains converge on the same destination through vacuum squares:
```
A → X → M
B → Y → M
```

This creates a design tension:
- **Principle of Largest Effect** wants both moves to succeed
- **Principle of Fairness** has no intrinsic ordering for A vs B
- **Weakest Precondition** must determine if this *must* be a conflict

**Solution**: Make merge handling **configurable** via game modes:

1. **DetectAndConflict**: Treat merges as conflicts (strict weakest precondition)
2. **BunchBeforeMerge**: Pieces stop one square before the merge point M; M remains empty
3. **BunchedStacking** (@Grissess): Pieces "stack up" along chains; first-to-arrive gets M, others compress behind
4. **Annihilate**: Tactical "denial" mode (all converging pieces destroyed)

### Cycle Semantics

When moves form cycles (e.g., A→B, B→C, C→A), there's tension between:
- **Principle of Largest Effect**: All edges should "fire"
- **Physical intuition**: What does it mean for pieces to rotate?

**Observations** (ember & @Grissess):
- "Every edge fired once" justifies both motion (>2-cycles) and stasis (2-cycles)
- {A→B, B→A} composes into {A→A}: moves succeed but no material displacement
- All moves should be considered "successful" even if pieces don't move

**Solution**:
- **1-cycles** (A→A): Forbidden by structural validation in `propose_move` (from==to)
- **2-cycles** (A→B, B→A): **Always** stay in place - unambiguous composition
- **>2-cycles**: Make behavior **configurable**:
  1. **RotatePieces**: Pieces advance one position around the cycle
  2. **NoMovement**: All moves succeed, pieces remain in place (extends 2-cycle semantics)

### Empty Cycles

An empty cycle cannot "pull" pieces into it. This follows from:
- **Principle of Largest Effect**: If allowing entry creates ambiguity, don't allow it
- **Physical intuition**: Vacuum squares can't exert force on pieces
- **Weakest Precondition**: Entry point into empty cycle has no canonical ordering

## Attributed Innovations

### BunchedStacking Mode (@Grissess)

Pieces along a chain "compress" toward the end:
- Given a chain X1 → X2 → ... → Xn with m pieces
- Process in reverse path order: X(n-1)→Xn first, then X(n-2)→X(n-1), etc.
- Each piece moves as far along the path as it can
- Result: m pieces occupy X(n-m) through Xn (compressed at the end)

This creates "traffic jams" at popular destinations while respecting the Principle of Fairness (compression order emerges from topology, not player IDs).

## Open Questions

1. **Cycle length semantics**: Should 1-cycles (self-loops) be treated specially?
2. **Merge point orderings**: Could path length provide a fair ordering for merges?
3. **Hybrid modes**: Should we support combinations (e.g., "bunch with 2-piece limit, then conflict")?
4. **Column rule justification**: Is there a principle-based argument for column-first tie-breaking?

## Historical Notes

These design principles emerged from:
- The original 2-player game (vacuum chains, cycles)
- The n-player extension (merging pathways, fairness concerns)
- Collaboration between ember and @Grissess
- Implementation using Tarjan SCC graph decomposition

The "conceptual bug" that catalyzed this analysis: conflict mechanics weren't a sufficient weakest precondition to ensure sensicality in n>2 games, leading to pieces vanishing at merge points.
