import cmd, shlex, json

import model

class GameUI(cmd.Cmd):
    def preloop(self):
        self.init_state()

    def postcmd(self, stop, line):
        if stop:
            return stop
        self.show_state()
        self.dump_events()

    def init_state(self, players=2):
        print('Initializing a', players, 'player game state')
        plebs = [model.Plebeian(i+1) for i in range(players)]
        self.game = model.Game(*plebs)
        self.show_state()

    PC_CHARS = ['.', 'W', 'B', 'A']

    def show_state(self, real_values=False):
        for colid in reversed(range(self.game.board.height)):
            print('{:2}'.format(colid), end=' ')
            for rowid in range(self.game.board.width):
                pc = self.game.board[rowid, colid]
                if real_values:
                    print('{:016x}'.format(pc), end=' ')
                else:
                    s = self.PC_CHARS[pc & 0xF]
                    s = ('!' if pc & model.Board.PC_F_CONFLICT else '_') + s
                    s += 'G' if pc & model.Board.PC_F_GOAL else '_'
                    print(s, end=' ')
            print()

    def dump_events(self):
        for pleb in self.game.plebeians:
            evs = pleb.Events()
            if evs:
                print('To Pleb', pleb.id)
                for ev in evs:
                    print('', ev.Serialize())

    def show_oev(self, oev):
        print(oev.serialize())

    def do_init(self, line):
        '''(Re)initializes the game state for the number of players given as an argument (2 or 4).'''
        self.init_state(int(line))

    def do_show(self, line):
        '''Shows the board state. If an arg is given, shows raw values.'''
        self.show_state(bool(line))

    def do_move(self, line):
        '''Posts a move from player 1-4 (first argument) from the source coordinate (2nd, 3rd) to the destination (4th, 5th).'''
        parts = shlex.split(line)
        plnum = int(parts[0])
        srcpair = tuple(map(int, parts[1:3]))
        dstpair = tuple(map(int, parts[3:5]))
        self.game.Handle(model.Move(self.game.plebeians[plnum - 1], srcpair, dstpair))

    def do_show_moves(self, line):
        '''Shows pending moves in the game state.'''
        if self.game.pending_moves:
            for pleb, move in self.game.pending_moves.items():
                print('Player', pleb.id, 'wants to move from', move[0], 'to', move[1])
        else:
            print('No pending moves yet.')

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
        print('The agent wants to move from', src, 'to', dst)
        if not self.game.board.Move(src, dst):
            print('Movement failed.')

    def do_set_board(self, line):
        '''Sets a board entry to a given value. Check the bitflags in the Board for more information. Args are row, col, value.'''
        parts = shlex.split(line)
        pos = tuple(map(int, parts[0:2]))
        val = int(parts[2])
        self.game.board[pos] = val

    def do_shell(self, line):
        '''Runs Python in the shell's namespace. Notably, game is a Game object.'''
        exec(line, globals(), self.__dict__)

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
        json.dump(self.game.board.columns, open(line, 'w'), indent=4)

    def do_load(self, line):
        '''Loads the state from the named file (not shell quoted).'''
        self.game.board.columns = json.load(open(line))

if __name__ == '__main__':
    shell = GameUI()
    shell.cmdloop()
