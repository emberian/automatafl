# automatafl
An implementation of the Automatafl table-top game

## Running

1. Start the `ws_server.py` WebSocket server.

2. Open `game.html` in a web browser. You can, e.g., serve this over HTTP; it will connect back to the same address used for HTTP.

3. Every client starts in their own game session. Have one player join the game of another player, and ensure each player clicks the "Be player 1" or "Be player 2" buttons. (As many other players who care to can connect, observe, and chat.)

4. Click on the board square, first source, then destination, to designate a move. You may change your move before all moves are in, but the game advances as soon as that happens. Conflicted squares are marked in red and may not be selected.

