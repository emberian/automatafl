# automatafl-rs

This is a not-yet-production-quality implementation of automatafl.

## Game Rules

This section is the canonical, Creator-Approved ruleset because said Creator lost their creds that got them into the hidden wiki that contains the No-Longer-Canonical ruleset. 

### Gameplay

Like many other games, Automatafl is turn-based; unlike the *vast majority* of other turn-based games, *all players' turns happen simultaneously* Afterward, the Automaton's "step" is computed by a convoluted set of rules with various priorities. Thus, each turn has two phases.

#### Initial Setup

Before beginning:

* In a two-player game, each player picks two corners that are in the same row.
* In a four-player game, each player picks exactly one corner.

#### Win Condition

When the Automaton moves into a corner, the game is won by whomever owns the corner. 

#### Move Entry Phase

All players secretly write down a move, which is a sequence of two coordinates (the first is a source, the second a destination) such that the source is not equal to the destination, and at least one of the coordinate axes (X or Y) is shared between the source and the destination (that is, it would make a valid rook move on an empty board). When all players have prepared a move, all the moves are revealed simultaneously.

*Before* moves "resolve", players must check for *conflicts*  a conflict occurs if:

* Multiple players specify the same source, and a piece is at that source; or
* Multiple players specify the same destination.

In the event of a conflict, all players involved in the conflict must invalidate their previous move and prepare another move. It is illegal to specify as a source or destination the *exact* coordinate which was conflicted upon (that is, in a source conflict, that piece becomes immovable; in a destination conflict, that square can no longer be moved to); this is often indicated with a temporary marker, such as overturning a conflicted source piece, or putting a coin on the conflicted destination. After all involved players have prepared their respective moves, they are revealed simultaneously, and, if needed, the conflict resolution will recurse (from "before moves resolve" above), possibly with only a subset of involved players.

*Only* once conflict resolution is complete do moves resolve, as follows:

1. All temporary markers used in conflict resolution are cleared.
2. All pieces specified as the source of a move are temporarily removed from the board, remembering the original position.
3. Each piece removed is placed in the specified destination, but *only* if no other piece (which is not in the process of being moved) is on the straight-line path between the source and the destination (otherwise, the piece is placed at its original position). In particular, it is legal to specify a move that initially "goes through" another piece; should that other piece be moved by another player, the move will succeed.
  * As a particular erratum, if a piece is moved into a square which is the source of another move, the piece participates in the move twice. That is, sort the moves topologically, with each move being an edge. Cycles are permissible--the piece simply doesn't move in this case.
4. After all these resolve, any remaining move is simply marked "invalid", and causes no change in state.

#### Automaton Step Phase

If all went well, the board is in a new state where a number of pieces not greater than the number of players have moved--which means it is now time for Automatafl's *most* distinct phase, with all the fun of a poker hand and a cellular automaton.

The Automaton position is the most important position for all of the following considerations. In particular, it suffices to determine the four nearest pieces along each positive and negative axis and their distances, or the absence of such pieces. When multiple axes conflict, the movement rule closer to first on this list (with the *lower* priority number) overrides; if the same movement rule applies, refer to that rule's text for how to resolve the conflict. In particular, the Automaton only ever moves by at most one step in a cardinal direction. After the Automaton moves, the Win Condition is checked; if no one has yet won, the game resumes from the next turn's Move Entry Phase.

##### Priority 1: Opposing Pairs

The single highest priority for the Automaton is to move toward an attractor piece and away from a repulsor *on the same axis* as long as there is *an empty space* in the direction of the attractor (otherwise the axis is invalidated, and the other axis considered). Should both axes have opposing pairs, the Automaton prefers to move along the axis toward the closer attractor; if the attractor are equidistant, the Automaton prefers to move along the axis away from the closer repulsor. Should *that* be equivalent too, the *column rule* is applied <small>(arguably a bug with the initial prototype, but we're going to stick with it I guess)</small> whereupon the Automaton prefers to move along the column instead of the row. <small>Since it is a dubious rule, some people have suggested having the Automaton "freeze" for that step altogether, and thus this is a selectable preference in every game. Seriously just play with the column rule though, it's fine.</small>

##### Priority 2: From Repulsor

The next highest priority for the Automaton to move is away from a repulsor *if an empty space exists in the direction opposite the closest repulsor on that axis* if either (1) both closest pieces on that axis are repulsor, or (2) only one repulsor is visible on that axis. If (1) applies and both repulsor are equidistant, this rule is removed from consideration on that axis. Should both axes apply, the Automaton moves away from the closest repulsor in *all* directions; if the closest pieces are on different axes, the *column rule* applies, as above.

##### Priority 3: Toward Attractor

The next highest priority for the Automaton is to move toward a attractor *with an empty space* toward the closest attractor on that axis, if either (1) both closest pieces on that axis are attractor, or (2) only one attractor is "visible" (reachable from the Automaton) on that axis. If (1) applies and both attractor are equidistant, this rule is removed from consideration on that axis. Should both axes apply, the Automaton moves toward the closest attractor in *all* directions; if the closest pieces are on different axes, the *column rule* applies, as above.

##### Priority 4: Fallback

If none of the higher priorities above apply, no movement is considered for that axis. If neither axis has a considerable move, the Automaton does not move for this phase.

## Possible Extensions (work for the reader)

- Hyperautomatafl: 3 or more dimensions
- Multiautomatafl: more than one automaton
- Flingautomatafl: make the grid continuous and model momentum
- More interesting particle influence. Maybe they exert different forces on different dimensions? Maybe you can rotate groups of particles?
- More interesting particle movement. Maybe you can move any particle anywhere, barring conflict. Maybe have different particles have chess rules for them.