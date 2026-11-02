To elaborate in another way:

1.  All temporary markers used in conflict resolution are cleared.

2.  All revealed moves are treated as a single, interconnected system of pathways. A move from square A to square B creates a one-way path from A to B for this turn. These paths can form long chains (A → B → C) or even cycles (A → B → A).

3.  All pieces that start the turn on a source square of any move are identified. These pieces are then "picked up" from the board, remembering where they came from.

4.  The final destination for each picked-up piece is calculated based on the full structure of the pathways. This happens for all pieces simultaneously, with the following outcomes:

        * Chains: A piece follows a chain of moves. If Player 1 moves a piece from A → B, and Player 2 makes a move from B → C (even if B was empty!), the piece from A will ultimately land on C. This allows for powerful team plays and "optimistic" moves where you move from a square you expect an ally to move a piece to. If multiple pieces start in the same chain (e.g., a piece on A and a piece on B in a chain A → B → C), they move forward like a caterpillar: the piece from A moves to B's original spot, and the piece from B moves to C.

        * Cycles: If pieces are on squares that form a cycle (e.g., moves are A → B, B → C, and C → A), all pieces rotate one position along the cycle.

        * Blocked Moves: A move is blocked only if an unmoving piece is on the direct path between its source and destination. Pieces that are also being moved do not block paths. If a move is blocked, the piece is returned to its original square.

        * Special Case (Empty Cycles): A piece moved into a cycle of squares that were all empty at the start of the turn will not enter the cycle. The move is nullified, and the piece stays in its original position. An empty cycle cannot "pull" a new piece into it.

5.  Once all final destinations are calculated, the pieces are placed on the board in their new positions. Any move that was blocked or nullified results in no change.