import asyncio, asyncio, cmd, shlex, json, traceback, re, logging
from collections import defaultdict

import websockets, websockets.server

import model

logger = logging.getLogger('websockets.server')
logger.addHandler(logging.StreamHandler())

def safe_filename(fn):
    return re.sub('[^a-zA-Z0-9]', '_', fn)

def setup_game(*plebs):
#    return model.Game(*plebs)
    return model.Game(*plebs, setup=[
        [2, 0, 1, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 3, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 1, 0, 2],
    ], goals=[[(0, 0), (4, 0)], [(0, 4), (4, 4)]])

class GameState(cmd.Cmd):
    class StdoutEmulator(object):
        def __init__(self, gs):
            self.gs = gs

        def write(self, s):
            self.gs.cli.tran.send(json.dumps({'msg': 'stdout', 'value': s}))

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

    async def print(self, obj):
        try:
            await self.cli.tran.send(json.dumps(obj))
        except websockets.ConnectionClosed:
            await self.postloop()

    async def broadcast(self, obj, no_players=False):
        old_cli = self.cli
        for cli in self.clis:
            if no_players and self.pleb_id_for(cli) is not None:
                continue
            self.cli = cli
            await self.print(obj)
        self.cli = old_cli

    async def init_state(self, players=2):
        plebs = [model.Plebeian(i+1) for i in range(players)]
        self.game = setup_game(*plebs)
        await self.print({'msg': 'init', 'players': players, 'width': self.game.board.width, 'height': self.game.board.height})
        await self.show_state()

    async def preloop(self):
        await self.init_state()
        await self.prompt()

    async def postloop(self):
        self.clis.discard(self.cli)
        if not self.clis:
            return
        if self.first_cli == self.cli:
            self.first_cli = self.clis.pop()
            self.clis.add(self.first_cli)

    async def precmd(self, line):
        return line

    async def default(self, line):
        await self.print({'msg': 'error', 'error': 'unknown command', 'line': line})

    async def onecmd(self, line):
        cmd, arg, line = self.parseline(line)
        if cmd:
            return await getattr(self, 'do_'+cmd, self.default)(arg)
        return await self.emptyline()

    async def postcmd(self, stop, line):
        if stop:
            return stop
        await self.dump_events()
        await self.show_state()

    async def emptyline(self):
        pass

    async def prompt(self):
        await self.print({
            'msg': 'ready',
            'name': self.cli.name,
            'room': self.first_cli.name,
            'pleb': self.pleb_id_for(self.cli),
        })

    PC_CHARS = ['.', 'W', 'B', 'A']

    async def broadcast_state(self):
        await self.broadcast({'msg': 'state', 'columns': self.game.board.columns, 'width': self.game.board.width, 'height': self.game.board.height})

    async def show_state(self):
        await self.print({'msg': 'state', 'columns': self.game.board.columns, 'width': self.game.board.width, 'height': self.game.board.height})

    async def dump_events(self):
        for ev in self.game.GlobalEvents():
            await self.broadcast(ev.Serialize(), True)
        old_cli = self.cli
        for pleb in self.game.plebeians:
            evs = pleb.Events()
            if evs:
                self.cli = self.pleb_to_cli.get(pleb.id)
                if self.cli is not None:
                    for ev in evs:
                        print('ev:', ev.Serialize())
                        await self.print(ev.Serialize())
        self.cli = old_cli

    async def do_goals(self, line):
        '''Display info about goals.'''
        goals = defaultdict(list)
        for rowid in range(self.game.board.height):
            for colid in range(self.game.board.width):
                cell = self.game.board[rowid, colid]
                if cell & model.Board.PC_F_GOAL:
                    goals[(cell >> 8) & 0xFF].append((colid, rowid))
        await self.print({'msg': 'goals', 'goals': goals})

    async def do_be_p(self, line):
        '''Become the nth player of this game (arg n).'''
        pleb = int(line)
        self.pleb_to_cli[pleb] = self.cli
        await self.broadcast({'msg': 'be_p', 'name': self.cli.name, 'pleb': pleb})

    async def do_state(self, line):
        await self.show_state()

    async def do_say(self, line):
        '''Say something to the other players in this room.'''
        await self.broadcast({'msg': 'say', 'name': self.cli.name, 'text': line})

    async def do_players(self, line):
        '''Print out players of the current game.'''
        stat = {}
        for cli in self.clis:
            pid = self.pleb_id_for(cli)
            stat[cli.name] = pid
        await self.print({'msg': 'players', 'players': stat})

    async def do_init(self, line):
        '''(Re)initializes the game state for the number of players given as an argument (2 or 4).'''
        await self.init_state(int(line))

    async def do_help(self, line):
        if line:
            meth = getattr(self, line, None)
            if meth is not None and hasattr(meth, '__doc__'):
                await self.print({'msg': 'help', 'cmd': line, 'help': meth.__doc__})
            else:
                await self.print({'msg': 'help', 'cmd': line, 'error': 'no help for that topic or command'})
        else:
            all_cmds = [i[3:] for i in dir(self) if i.startswith('do_')]
            await self.print({'msg': 'help', 'list': all_cmds})

    async def do_show(self, line):
        '''Shows board state (no op).'''
        pass

    async def do_move(self, line):
        '''Posts a move from the source coordinate (1st, 2nd) to the destination (3rd, 4th).'''
        pleb = self.current_pleb()
        if pleb is None:
            await self.print({'msg': 'move', 'error': 'not a player'})
            return
        parts = shlex.split(line)
        srcpair = tuple(map(int, parts[0:2]))
        dstpair = tuple(map(int, parts[2:4]))
        self.game.Handle(model.Move(pleb, srcpair, dstpair))

#    def do_show_moves(self, line):
#        '''Shows pending moves in the game state.'''
#        if self.game.pending_moves:
#            for pleb, move in self.game.pending_moves.items():
#                self.print('Player', pleb.id, 'wants to move from', move[0], 'to', move[1])
#        else:
#            self.print('No pending moves yet.')

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


#    def do_agent_step(self, line):
#        '''Steps the Agent against the current board state.'''
#        src, dst = self.game.board.AgentStep()
#        self.print('The agent wants to move from', src, 'to', dst)
#        if not self.game.board.Move(src, dst):
#            self.print('Movement failed.')
#
#    def do_set_board(self, line):
#        '''Sets a board entry to a given value. Check the bitflags in the Board for more information. Args are row, col, value.'''
#        parts = shlex.split(line)
#        pos = tuple(map(int, parts[0:2]))
#        val = int(parts[2])
#        self.game.board[pos] = val

#    def do_shell(self, line):
#        '''Runs Python in the shell's namespace. Notably, game is a Game object.'''
#        exec(line, globals(), self.__dict__)

#    def do_EOF(self, line):
#        '''Run whenever the input line contains EOF. Alias for quit.'''
#        return self.do_quit(line)
#
#    def do_q(self, line):
#        '''Alias for quit.'''
#        return self.do_quit(line)
#
#    def do_quit(self, line):
#        '''Exits the interpreter.'''
#        return True

    async def do_save(self, line):
        '''Saves the state to the name file (not shell quoted).'''
        fname = safe_filename(line)
        json.dump(self.game.board.columns, open('states/' + fname, 'w'), indent=4)
        await self.broadcast({'msg': 'save', 'as': fname})

    async def do_load(self, line):
        '''Loads the state from the named file (not shell quoted).'''
        fname = safe_filename(line)
        self.game.board.columns = json.load(open('states/' + safe_filename(line)))
        await self.broadcast({'msg': 'load', 'from': fname})

    async def do_set_name(self, line):
        '''Set the name other players will know you by.'''
        if line in type(self.cli).ALL_CLIENTS:
            await self.print({'msg': 'set_name', 'error': 'already in use'})
            return
        del type(self.cli).ALL_CLIENTS[self.cli.name]
        self.cli.name = line
        type(self.cli).ALL_CLIENTS[self.cli.name] = self.cli
        await self.print({'msg': 'set_name', 'name': self.cli.name})

    async def do_join_game(self, line):
        '''Join another player's game (player name is argument).'''
        cli = GameServerProtocol.ALL_CLIENTS.get(line)
        if cli is None:
            await self.print({'msg': 'join_game', 'error': 'game does not exist'})
            return
        if cli.state is self:
            return  # Nothing to do
        self.cli.state = cli.state
        cli.state.clis.add(self.cli)
        self.clis.discard(self.cli)
        cli.state.cli = self.cli
        await cli.state.broadcast({'msg': 'join_game', 'cli': self.cli.name})

    async def do_games(self, line):
        games = set()
        for cli in GameServerProtocol.ALL_CLIENTS.values():
            if cli.state in games:
                continue
            games.add(cli.state)
        await self.print({'msg': 'games', 'games': {i.first_cli.name: len(i.clis) for i in games}})

class GameServerProtocol(asyncio.Protocol):
    ALL_CLIENTS = {}
    CLIENT_NUM = 1

    async def connection_made(self, tran):
        self.tran = tran
        self.line_buf = ''
        self.name = 'Player {}'.format(self.CLIENT_NUM)
        self.state = GameState(self)
        await self.state.preloop()
        type(self).CLIENT_NUM += 1
        self.ALL_CLIENTS[self.name] = self

    async def connection_lost(self, exc):
        del self.ALL_CLIENTS[self.name]
        self.state.cli = self
        await self.state.postloop()

    async def data_received(self, data):
        self.line_buf += data
        did_one = False
        while True:
            line, sep, next = self.line_buf.partition('\n')
            if sep:
                self.state.cli = self
                try:
                    line = await self.state.precmd(line)
                    stop = await self.state.onecmd(line)
                    stop = await self.state.postcmd(stop, line)
                    if stop:
                        await self.tran.close()
                except Exception as e:
                    traceback.print_exc()
                    await self.tran.send(json.dumps({'msg': 'error', 'type': type(e).__name__, 'args': list(e.args)}))
                self.line_buf = next
                did_one = True
            else:
                break
        if did_one:
            await self.state.prompt()

async def handle_connection(ws, uri):
    gsp = GameServerProtocol()
    await gsp.connection_made(ws)
    try:
        while True:
            msg = await ws.recv()
            await gsp.data_received(msg)
    except websockets.ConnectionClosed:
        print('Connection closed.')
    finally:
        await gsp.connection_lost(None)

loop = asyncio.get_event_loop()
coro = websockets.server.serve(handle_connection, port=8080)
loop.run_until_complete(coro)
loop.run_forever()
