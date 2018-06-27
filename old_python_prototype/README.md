# automatafl
An implementation of the Automatafl table-top game

## Running

1. Start the `ws_server.py` WebSocket server.

2. Open `game.html` in a web browser. You can, e.g., serve this over HTTP; it will connect back to the same address used for HTTP.

3. Every client starts in their own game session. Have one player join the game of another player, and ensure each player clicks the "Be player 1" or "Be player 2" buttons. (As many other players who care to can connect, observe, and chat.)

4. Click on the board square, first source, then destination, to designate a move. You may change your move before all moves are in, but the game advances as soon as that happens. Conflicted squares are marked in red and may not be selected.

## Game Rules

(This section is copied, lightly modified, from our internal documentation.)

### Gameplay

Like many other games, Automatafl is turn-based; unlike the *vast majority* of other turn-based games, *all players' turns happen simultaneously* Afterward, the Agent's "step" is computed by a very convoluted set of rules with various priorities. Thus, each turn has two phases.

#### Initial Setup

Before beginning:

* In a two-player game, each player picks two corners that are in the same row.
* In a four-player game, each player picks exactly one corner.

#### Win Condition

When the Agent moves into a corner, the game is won by whomever owns the corner. 

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

#### Agent Step Phase

If all went well, the board is in a new state where a number of pieces not greater than the number of players has moved--which means it is now time for Automatafl's *most* distinct phase, with all the fun of a poker hand and a cellular automaton.

The Agent position is the most important position for all of the following considerations. In particular, it suffices to determine the four nearest pieces along each positive and negative axis and their distances, or the absence of such pieces. When multiple axes conflict, the movement rule closer to first on this list (with the *lower* priority number) overrides; if the same movement rule applies, refer to that rule's text for how to resolve the conflict. In particular, the Agent only ever moves by at most one step in a cardinal direction. After the Agent moves, the Win Condition is checked; if no one has yet won, the game resumes from the next turn's Move Entry Phase.

##### Priority 1: Opposing Pairs

The single highest priority for the Agent is to move toward a white piece and away from a black piece *on the same axis* as long as there is *an empty space* in the direction of the white piece (otherwise the axis is invalidated, and the other axis considered). Should both axes have opposing pairs, the Agent prefers to move along the axis toward the closer white; if the white pieces are equidistant, the Agent prefers to move along the axis away from the closer black. Should *that* be equivalent too, the *column rule* is applied (arguably a bug with the [reference implementation](https://github.com/cmr/automatafl/blob/master/model.py)  whereupon the Agent prefers to move along the column instead of the row. Since it is a dubious rule, some people have suggested having the Agent "freeze" for that step altogether.

##### Priority 2: From Black

The next highest priority for the Agent to move is away from a black piece *if an empty space exists in the direction opposite the closest black piece on that axis* if either (1) both closest pieces on that axis are black, or (2) only one black piece is visible on that axis. If (1) applies and both black pieces are equidistant, this rule is removed from consideration on that axis. Should both axes apply, the Agent moves away from the closest black piece in *all* directions; if the closest pieces are on different axes, the *column rule* applies, as above.

##### Priority 3: Toward White

The next highest priority for the Agent is to move toward a white piece *with an empty space* toward the closest white on that axis, if either (1) both closest pieces on that axis are white, or (2) only one white piece is "visible" (reachable from the Agent) on that axis. If (1) applies and both white pieces are equidistant, this rule is removed from consideration on that axis. Should both axes apply, the Agent moves toward the closest white piece in *all* directions; if the closest pieces are on different axes, the *column rule* applies, as above.

##### Priority 4: Fallback

If none of the higher priorities above apply, no movement is considered for that axis. If neither axis has a considerable move, the Agent does not move for this phase.
