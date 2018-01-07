import asyncio, asyncio, cmd, shlex, json, traceback, re

import model

def safe_filename(fn):
    return re.sub('[^a-zA-Z0-9]', '_', fn)

class GameState(cmd.Cmd):
    class StdoutEmulator(object):
        def __init__(self, gs):
            self.gs = gs

        def write(self, s):
            self.gs.cli.tran.write(s.encode('utf8'))

    def __init__(self, cli, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.clis = set((cli,))
        self.cli = cli  # Also set by Protocol
        self.first_cli = cli
        self.pleb_to_cli = {}
        self.stdout = self.StdoutEmulator(self)

    def pleb_id_for(self, cli):
        for id, pcli in self.pleb_to_cli.items():
            if pcli == cli:
                return id
        return None

    def pleb_for(self, cli):
        pid = self.pleb_id_for(cli)
        if pid is None:
            return None
        for pleb in self.game.plebeians:
            if pleb.id == pid:
                return pleb

    def current_pleb(self):
        return self.pleb_for(self.cli)

    def print(self, *args, end='\n', sep=' ', flush=False):
        # TODO: flush
        self.cli.tran.write((sep.join((i if isinstance(i, str) else repr(i)) for i in args) + end).encode('utf8'))

    def broadcast(self, *args, end='\n', sep=' '):
        old_cli = self.cli
        for cli in self.clis:
            if self.pleb_id_for(cli) is not None:
                continue
            self.cli = cli
            self.print(*args, end=end, sep=sep)
        self.cli = old_cli

    def init_state(self, players=2):
        self.print('Initializing a', players, 'player game state')
        plebs = [model.Plebeian(i+1) for i in range(players)]
        self.game = model.Game(*plebs)
        self.show_state()

    def preloop(self):
        self.init_state()
        self.prompt()

    def postloop(self):
        self.clis.remove(self.cli)
        if not self.clis:
            return
        if self.first_cli == self.cli:
            self.first_cli = self.clis.pop()
            self.clis.add(self.first_cli)

    def postcmd(self, stop, line):
        if stop:
            return stop
        self.show_state()
        self.dump_events()

    def emptyline(self):
        pass

    def prompt(self):
        s = self.cli.name
        if self.first_cli.name != self.cli.name:
            s += '@' + self.first_cli.name
        pid = self.pleb_id_for(self.cli)
        if pid is not None:
            s += ' [Player {}]'.format(pid)
        self.print('{}> '.format(s), end='', flush=True)

    PC_CHARS = ['.', 'W', 'B', 'A']

    def show_state(self, real_values=False):
        for colid in reversed(range(self.game.board.width)):
            self.print('{:2}'.format(colid), end=' ')
            for rowid in range(self.game.board.height):
                pc = self.game.board[rowid, colid]
                if real_values:
                    self.print('{:016x}'.format(pc), end=' ')
                else:
                    s = self.PC_CHARS[pc & 0xF]
                    s = ('!' if pc & model.Board.PC_F_CONFLICT else '_') + s
                    s += 'G' if pc & model.Board.PC_F_GOAL else '_'
                    self.print(s, end=' ')
            self.print()

    def dump_events(self):
        for ev in self.game.GlobalEvents():
            self.broadcast(ev.Serialize())
        for pleb in self.game.plebeians:
            evs = pleb.Events()
            if evs:
                self.cli = self.pleb_to_cli.get(pleb.id)
                if self.cli is not None:
                    for ev in evs:
                        self.print(ev.Serialize())

    def do_goals(self, line):
        '''Display info about goals.'''
        for rowid in range(self.game.board.height):
            for colid in range(self.game.board.width):
                cell = self.game.board[rowid, colid]
                if cell & model.Board.PC_F_GOAL:
                    self.print('Pleb', (cell >> 8) & 0xFF, 'has a goal at', (colid, rowid))

    def do_be_p1(self, line):
        '''Become the first player of this game.'''
        self.pleb_to_cli[1] = self.cli

    def do_be_p2(self, line):
        '''Become the second player of this game.'''
        self.pleb_to_cli[2] = self.cli

    def do_players(self, line):
        '''Print out players of the current game.'''
        for cli in self.clis:
            pid = self.pleb_id_for(cli)
            self.print(cli.name, 'is', ('Player {}'.format(pid) if pid is not None else 'spectating'))

    def do_init(self, line):
        '''(Re)initializes the game state for the number of players given as an argument (2 or 4).'''
        self.init_state(int(line))

    def do_show(self, line):
        '''Shows the board state. If an arg is given, shows raw values.'''
        if line:  # Else postcmd picks this up
            self.show_state(True)

    def do_move(self, line):
        '''Posts a move from the source coordinate (1st, 2nd) to the destination (3rd, 4th).'''
        pleb = self.current_pleb()
        if pleb is None:
            self.print('You are not a player (try `be_p1`, `be_p2`)')
            return
        parts = shlex.split(line)
        srcpair = tuple(map(int, parts[0:2]))
        dstpair = tuple(map(int, parts[2:4]))
        self.game.Handle(model.Move(pleb, srcpair, dstpair))

    def do_show_moves(self, line):
        '''Shows pending moves in the game state.'''
        if self.game.pending_moves:
            for pleb, move in self.game.pending_moves.items():
                self.print('Player', pleb.id, 'wants to move from', move[0], 'to', move[1])
        else:
            self.print('No pending moves yet.')

#    def do_resolve(self, line):
#        '''Resolves all moves that are pending.'''
#        if len(self.game.pending_moves) != len(self.game.plebeians):
#            print('Not all players have entered moves.')
#            return
#        oevl = self.game.Resolve()
#        if oevl:
#            for oev in oevl:
#                self.show_oev(oev)
#                for pleb in oev.plebs:
#                    del self.game.pending_moves[pleb]
#                self.game.board[oev.square] &= model.Board.PC_F_CONFLICT
#                print('Conflicts were set.')
#        else:
#            print('No conflicts, moving everything and clearing conflicts.')
#            unique_moves = set()
#            for move in self.game.pending_moves.values():
#                unique_moves.add(move)
#            for src, dst in unique_moves:
#                if not self.game.board.Move(src, dst):
#                    print('Move from', src, 'to', dst, 'failed.')
#            for rowid in range(self.game.board.width):
#                for colid in range(self.game.board.height):
#                    self.game.board[rowid, colid] &= ~model.Board.PC_F_CONFLICT


    def do_agent_step(self, line):
        '''Steps the Agent against the current board state.'''
        src, dst = self.game.board.AgentStep()
        self.print('The agent wants to move from', src, 'to', dst)
        if not self.game.board.Move(src, dst):
            self.print('Movement failed.')

    def do_set_board(self, line):
        '''Sets a board entry to a given value. Check the bitflags in the Board for more information. Args are row, col, value.'''
        parts = shlex.split(line)
        pos = tuple(map(int, parts[0:2]))
        val = int(parts[2])
        self.game.board[pos] = val

#    def do_shell(self, line):
#        '''Runs Python in the shell's namespace. Notably, game is a Game object.'''
#        exec(line, globals(), self.__dict__)

    def do_EOF(self, line):
        '''Run whenever the input line contains EOF. Alias for quit.'''
        return self.do_quit(line)

    def do_q(self, line):
        '''Alias for quit.'''
        return self.do_quit(line)

    def do_quit(self, line):
        '''Exits the interpreter.'''
        return True

    def do_save(self, line):
        '''Saves the state to the name file (not shell quoted).'''
        json.dump(self.game.board.columns, open('states/' + safe_filename(line), 'w'), indent=4)

    def do_load(self, line):
        '''Loads the state from the named file (not shell quoted).'''
        self.game.board.columns = json.load(open('states/' + safe_filename(line)))

    def do_set_name(self, line):
        '''Set the name other players will know you by.'''
        del type(self.cli).ALL_CLIENTS[self.cli.name]
        self.cli.name = line
        type(self.cli).ALL_CLIENTS[self.cli.name] = self.cli
        self.print('You are now known as', self.cli.name)

    def do_join_game(self, line):
        '''Join another player's game (player name is argument).'''
        cli = GameServerProtocol.ALL_CLIENTS.get(line)
        if cli is None:
            self.print('No client with that name.')
            return
        self.cli.state = cli.state
        cli.state.clis.add(self.cli)
        self.clis.discard(self.cli)
        self.print('Joined.')
        cli.state.cli = self.cli

    def do_games(self, line):
        games = set()
        for cli in GameServerProtocol.ALL_CLIENTS.values():
            if cli.state in games:
                continue
            games.add(cli.state)
            self.print('-', cli.state.first_cli.name)

class GameServerProtocol(asyncio.Protocol):
    ALL_CLIENTS = {}
    CLIENT_NUM = 1

    def connection_made(self, tran):
        self.tran = tran
        self.line_buf = b''
        self.name = 'Player {}'.format(self.CLIENT_NUM)
        self.state = GameState(self)
        self.state.preloop()
        type(self).CLIENT_NUM += 1
        self.ALL_CLIENTS[self.name] = self

    def connection_lost(self, exc):
        del self.ALL_CLIENTS[self.name]
        self.state.cli = self
        self.state.postloop()

    def data_received(self, data):
        self.line_buf += data
        did_one = False
        while True:
            line, sep, next = self.line_buf.partition(b'\n')
            if sep:
                self.state.cli = self
                line = line.decode('utf8')
                try:
                    line = self.state.precmd(line)
                    stop = self.state.onecmd(line)
                    stop = self.state.postcmd(stop, line)
                    if stop:
                        self.tran.write_eof()
                except Exception:
                    self.tran.write(('*** Exception during command:' + traceback.format_exc()).encode('utf8'))
                self.line_buf = next
                did_one = True
            else:
                break
        if did_one:
            self.state.prompt()

loop = asyncio.get_event_loop()
coro = loop.create_server(GameServerProtocol, '', 2323)
loop.run_until_complete(coro)
loop.run_forever()
