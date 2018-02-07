'''
automatafl -- Automatafl
model -- Datamodel

Defines the main datamodel for automatafl.
'''

from collections import defaultdict
from queue import Queue, Empty

class Event():
    pass

class IEvent(Event):
    pass

class Move(IEvent):
    def __init__(self, pleb, srcpair, dstpair):
        self.pleb = pleb
        self.srcpair = srcpair
        self.dstpair = dstpair

class OEvent(Event):
    pass

class MoveAck(OEvent):
    def __init__(self, pleb):
        self.pleb = pleb

    def Serialize(self):
        return {"kind": "MOVE_ACK", "pleb": self.pleb.id}


class MoveInvalid(OEvent):
    POS_OCCUPIED        = 1
    POS_OOB             = 2
    POS_CONFLICT        = 3
    POS_CANT_MOVE_THAT  = 4
    POS_CANT_MOVE_THERE = 5
    POS_MOVE_LOCKED_IN  = 6
    POS_ILLEGAL         = 7
    GAME_OVER           = 8
    
    REASON_NAMES = {
        1: "POS_OCCUPIED",
        2: "POS_OOB",
        3: "POS_CONFLICT",
        4: "POS_CANT_MOVE_THAT",
        5: "POS_CANT_MOVE_THERE",
        6: "POS_MOVE_LOCKED_IN",
        7: "POS_ILLEGAL",
        8: "GAME_OVER",
    }

    def __init__(self, pleb, reason):
        assert(reason in [self.POS_MOVE_LOCKED_IN,
                          self.POS_OOB, self.POS_CONFLICT,
                          self.POS_CANT_MOVE_THAT,
                          self.POS_CANT_MOVE_THERE,
                          self.POS_OCCUPIED,
                          self.POS_ILLEGAL,
                          self.GAME_OVER])
        self.pleb = pleb
        self.reason = reason

    def Serialize(self):
        return {"kind": "MOVE_INVALID",
                "reason": MoveInvalid.REASON_NAMES[self.reason]}

class DoMove(OEvent):
    def __init__(self, pleb, src, dst, success):
        self.pleb = pleb
        self.src = src
        self.dst = dst
        self.success = success

    def Serialize(self):
        return {"kind": "MOVE",
                "pleb": self.pleb.id if self.pleb is not None else None,
                "src": self.src, "dst": self.dst,
                "success": self.success}

class Conflict(OEvent):
    def __init__(self, square, *plebs):
        self.square = square
        self.plebs = plebs

    def Serialize(self):
        return {"kind": "CONFLICT", "square": self.square, "plebs": [pleb.id for pleb in self.plebs]}

class TurnOver(OEvent):
    def __init__(self, asrc, adst, winner=None):
        self.asrc = asrc
        self.adst = adst
        self.winner = winner

    def Serialize(self):
        return {"kind": "TURN_OVER", "agent_src": self.asrc, "agent_dst": self.adst, "winner": self.winner}

class Game():
    ST_INIT = 0
    ST_WAITING = 1
    ST_RESOLVING = 2
    # Note: The internal lists are columns!
    DEFAULT_SETUP = [
        [2, 2, 0, 0, 2, 2, 2, 0, 0, 2, 2],
        [0, 0, 0, 1, 2, 2, 2, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1],
        [2, 2, 0, 0, 0, 3, 0, 0, 0, 2, 2],
        [1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 1, 2, 2, 2, 1, 0, 0, 0],
        [2, 2, 0, 0, 2, 2, 2, 0, 0, 2, 2],
    ]
    DEFAULT_GOALS = {
        2: [[(0, 0), (0, 10)], [(10, 0), (10, 10)]],
        4: [[(0, 0)], [(10, 0)], [(10, 10)], [(0, 10)]],
    }

    def __init__(self, *plebs, setup=None):
        if setup is None:
            setup = self.DEFAULT_SETUP
        assert len(plebs) in self.DEFAULT_GOALS.keys()
        self.pending_moves = {}
        self.locked = set()
        self.plebeians = list(plebs)
        self.board = Board(*setup)
        if setup is self.DEFAULT_SETUP:
            for plidx, goals in enumerate(self.DEFAULT_GOALS[len(plebs)]):
                for goal in goals:
                    self.board.SetPlayerGoal(goal, plebs[plidx])
        self.oq = Queue()

    def Handle(self, iev):
        return getattr(self, 'ev_'+type(iev).__name__, self.UnknownEvent)(iev)

    def UnknownEvent(self, iev):
        raise ValueError('Unknown input event %r'%(repr(iev),))

    def ev_Move(self, iev):
        # TODO: these should be in Board.CanMove
        if self.Winner() is not None:
            iev.pleb.Enqueue(MoveInvalid(iev.pleb, MoveInvalid.GAME_OVER))
        elif (tuple(iev.srcpair) == tuple(iev.dstpair)) or (not ((iev.srcpair[0] == iev.dstpair[0]) or (iev.srcpair[1] == iev.dstpair[1]))):
            iev.pleb.Enqueue(MoveInvalid(iev.pleb, MoveInvalid.POS_ILLEGAL))
        elif (self.board[iev.srcpair] & 0x0F) == Board.PC_AGENT:
            iev.pleb.Enqueue(MoveInvalid(iev.pleb, MoveInvalid.POS_CANT_MOVE_THAT))
        elif (self.board[iev.srcpair] & Board.PC_F_CONFLICT) != 0 or (self.board[iev.dstpair] & Board.PC_F_CONFLICT) != 0:
            iev.pleb.Enqueue(MoveInvalid(iev.pleb, MoveInvalid.POS_CONFLICT))
        elif iev.pleb in self.locked:
            iev.pleb.Enqueue(MoveInvalid(iev.pleb, MoveInvalid.POS_MOVE_LOCKED_IN))
        else:
            self.pending_moves[iev.pleb] = (iev.srcpair, iev.dstpair)
            self.Broadcast(MoveAck(iev.pleb))
            if len(self.pending_moves) == len(self.plebeians):
                self.Resolve()

    def Resolve(self):
        seen_pieces = dict()
        seen_dests = dict()
        conflicts = defaultdict(set)
        seen_moves = set()
        conflicting_plebs = set()
        for pleb, pending_move in self.pending_moves.items():
            if pending_move in seen_moves:
                continue
            seen_moves.add(pending_move)

            if pending_move[0] in seen_pieces:
                conflicts[pending_move[0]].add(pleb)
                conflicts[pending_move[0]].add(seen_pieces[pending_move[0]])
            else:
                seen_pieces[pending_move[0]] = pleb

            if pending_move[1] in seen_dests:
                conflicts[pending_move[1]].add(pleb)
                conflicts[pending_move[1]].add(seen_dests[pending_move[1]])
            else:
                seen_dests[pending_move[1]] = pleb

        if conflicts:
            # Generate the conflict events.
            conflicted_plebs = set()
            for conf_set in conflicts.values():
                conflicted_plebs |= conf_set
            for pleb in conflicted_plebs:
                del self.pending_moves[pleb]
            self.locked |= set(self.pending_moves.keys())
            for square, plebs in conflicts.items():
                self.Broadcast(Conflict(square, *plebs))
                self.board[square] |= Board.PC_F_CONFLICT
        else:
            self.CompleteMoves()
            self.Broadcast(TurnOver(*self.board.AgentStep(), self.Winner()))

    def CompleteMoves(self):
        propmap = dict(zip(self.pending_moves.values(), self.pending_moves.keys()))
        moves = set(self.pending_moves.values())
        for src, dst in moves:
            self.board[src] |= Board.PC_F_PASSABLE
            self.board[dst] |= Board.PC_F_PASSABLE

        success = {}
        while moves:
            for src, dst in moves:
                if self.board[src] & 0xF == Board.PC_EMPTY:
                    continue
                if self.board[dst] & 0xF != Board.PC_EMPTY:
                    continue
                success[src, dst] = self.board.Move(src, dst)
                moves.discard((src, dst))
                break
            else:
                break

        for move, succ in success.items():
            self.Broadcast(DoMove(propmap[move], move[0], move[1], succ))

        for move in moves:
            self.Broadcast(DoMove(propmap[move], move[0], move[1], False))

        self.pending_moves.clear()
        self.ClearState()

        src, dst = self.board.AgentStep()
        self.board[src] &= ~0xF
        self.board[dst] = (self.board[dst] & ~0xF) | Board.PC_AGENT
        self.Broadcast(DoMove(None, src, dst, True))

    def Winner(self):
        agent = self.board._FindAgent()
        acell = self.board[agent]
        if acell & Board.PC_F_GOAL:
            return acell >> 8
        return None

    def ClearState(self):
        self.locked.clear()
        for col in self.board.columns:
            for rowidx, cell in enumerate(col):
                col[rowidx] &= ~Board.PC_F_CONFLICT
                col[rowidx] &= ~Board.PC_F_PASSABLE

    def Broadcast(self, oev):
        self.oq.put(oev)
        for pleb in self.plebeians:
            pleb.Enqueue(oev)

    def GlobalEvents(self):
        ret = []
        while True:
            try:
                ret.append(self.oq.get_nowait())
            except Empty:
                return ret

    def Serialize(self):
        columns = []
        for column in self.board.columns:
            col = []
            for cell in column:
                c = {}
                c["occupant"] = Board.OCCUPANT_NAMES[cell & 0x0F]
                if cell & Board.PC_F_CONFLICT:
                    c["conflict"] = True
                if cell & Board.PC_F_GOAL:
                    c["goal"] = cell >> 8

                col.append(c)
            columsn.append(col)
        return columns

class Plebeian():
    def __init__(self, id):
        self.id = id
        self.oq = Queue()

    @staticmethod
    def ToID(obj):
        if isinstance(obj, Plebeian):
            return obj.id
        return obj

    def Enqueue(self, oev):
        self.oq.put(oev)

    def Events(self):
        ret = []
        while True:
            try:
                ret.append(self.oq.get_nowait())
            except Empty:
                return ret

class Board():
    PC_EMPTY = 0x00
    PC_WHITE = 0x01
    PC_BLACK = 0x02
    PC_AGENT = 0x03
    PC_F_CONFLICT = 0x10
    PC_F_GOAL = 0x20
    PC_F_PASSABLE = 0x40

    OCCUPANT_NAMES = {
        0: "EMPTY",
        1: "WHITE",
        2: "BLACK",
        3: "AGENT",
    }


    def __init__(self, *cols):
        self.columns = list(list(i) for i in cols)
        self.width = len(cols)
        self.height = len(cols[0])
        assert all(len(i) == len(cols[0]) for i in cols[1:])
        self.agent = None
        self._FindAgent()

    def _FindAgent(self):
        for colidx, col in enumerate(self.columns):
            for rowidx, row in enumerate(col):
                if (self[colidx, rowidx] & 0x0F) == self.PC_AGENT:
                    self.agent = (colidx, rowidx)
                    return self.agent

    def SetPlayerGoal(self, pair, pleb):
        print('SETPLAYERGOAL:', pair, 'for', pleb.id)
        pleb = Plebeian.ToID(pleb)
        cell = self[pair]
        cell = (cell & 0xFF) | self.PC_F_GOAL | (pleb << 8)
        self[pair] = cell
        print('SETPLAYERGOAL: New value is', hex(self[pair]))

    def CanMove(self, srcpair, dstpair):
        if not (self.InBoard(srcpair) and self.InBoard(dstpair)):
            print('CANMOVE: Not in board.')
            return False
        if (self[srcpair] & self.PC_F_CONFLICT):
            print('CANMOVE: Src is conflicted.')
            return False
        # NB: The following is also a sneaky conflict check
        if (self[dstpair] & self.PC_F_CONFLICT):
            print('CANMOVE: Dst is conflicted.')
            return False
        if not ((srcpair[0] == dstpair[0]) or (srcpair[1] == dstpair[1])):
            print('CANMOVE: Not a rank or file.')
            return False
        mins = (min(srcpair[0], dstpair[0]), min(srcpair[1], dstpair[1]))
        maxs = (max(srcpair[0], dstpair[0]), max(srcpair[1], dstpair[1]))
        squares = [(c, r) for c in range(mins[0], maxs[0] + 1) for r in range(mins[1], maxs[1] + 1)]
        for sq in squares:
            if not ((self[sq] & 0xF == self.PC_EMPTY) or self[sq] & self.PC_F_PASSABLE):
                print('CANMOVE: Nonempty/passable square at', sq)
                return False
        print('CANMOVE: OK.')
        return True

    def Move(self, srcpair, dstpair):
        if srcpair == dstpair:
            return True
        if self.CanMove(srcpair, dstpair):
            self[dstpair] = (self[dstpair] & ~0xF) | (self[srcpair] & 0x0F)
            self[srcpair] = (self[srcpair] & ~0x0F) | self.PC_EMPTY
            return True
        return False

    def InBoard(self, pair):
        return 0 <= pair[0] < self.width and 0 <= pair[1] < self.height

    def AgentStep(self):
        self._FindAgent()
        nearest = {}
        for axis in [(1, 0), (0, 1), (-1, 0), (0, -1)]:
            start = self.agent
            while True:
                start = (start[0] + axis[0], start[1] + axis[1])
                if not self.InBoard(start):
                    nearest[axis] = None
                    break
                if (self[start] & 0x0F) != self.PC_EMPTY:
                    nearest[axis] = start
                    break
        assert all((i is None) or (self[i] & 0x0F) != self.PC_AGENT for i in nearest.values())
        colpri, coldir = self._DoAgent(nearest[(1, 0)], nearest[(-1, 0)])
        rowpri, rowdir = self._DoAgent(nearest[(0, 1)], nearest[(0, -1)])
        print('DOAGENT: On columns, the priority is', hex(colpri), 'for direction', coldir)
        print('DOAGENT: On rows, the priority is', hex(rowpri), 'for direction', rowdir)
        colpair = self.agent[0] + coldir, self.agent[1]
        rowpair = self.agent[0], self.agent[1] + rowdir
        if colpri >= rowpri and self.InBoard(colpair):
            return self.agent, colpair
        if self.InBoard(rowpair):
            return self.agent, rowpair
        return self.agent, self.agent

    @staticmethod
    def L1Norm(p1, p2):
        return abs(p1[0] - p2[0]) + abs(p1[1] - p2[1])

    PRI_UNBAL = 0x30000
    PRI_AWAY = 0x20000
    PRI_TOWARD = 0x10000
    PRI_NONE = 0

    def _DoAgent(self, pospair, negpair):
        pospc = (self[pospair] & 0x0F if pospair is not None else self.PC_EMPTY)
        negpc = (self[negpair] & 0x0F if negpair is not None else self.PC_EMPTY)
        maxdist = max(self.width, self.height)
        print('DOAGENT: Pos/Neg pieces at', pospair, negpair, 'are', hex(pospc), hex(negpc))
        # (1) Check unbalanced pairs
        if (pospc, negpc) in [(self.PC_WHITE, self.PC_BLACK), (self.PC_BLACK, self.PC_WHITE)]:
            whpair = (pospair if pospc == self.PC_WHITE else negpair)
            blpair = (pospair if pospc == self.PC_BLACK else negpair)
            print('DOAGENT: Unbal, whpair', whpair, 'blpair', blpair)
            whdist = self.L1Norm(self.agent, whpair)
            bldist = self.L1Norm(self.agent, blpair)
            print('DOAGENT: Unbal, whdist', whdist, 'bldist', bldist)
            if whdist <= 1:
                return self.PRI_NONE, 0
            return self.PRI_UNBAL | ((maxdist - whdist) << 8) | bldist, (1 if whpair == pospair else -1)
        # (2) Check toward white
        # (2a) check both directions
        if pospc == self.PC_WHITE and negpc == self.PC_WHITE:
            posdist = self.L1Norm(self.agent, pospair)
            negdist = self.L1Norm(self.agent, negpair)
            print('DOAGENT: White pair, dists', posdist, negdist)
            if posdist == negdist:
                return self.PRI_NONE, 0
            if 1 in (posdist, negdist):
                return self.PRI_NONE, 0
            return self.PRI_TOWARD | (maxdist - min(posdist, negdist)), (1 if posdist < negdist else -1)
        # (2b) check for singular pieces
        if (pospc, negpc) in [(self.PC_WHITE, self.PC_EMPTY), (self.PC_EMPTY, self.PC_WHITE)]:
            pair = (pospair if pospc == self.PC_WHITE else negpair)
            dist = self.L1Norm(self.agent, pair)
            if dist == 1:
                return self.PRI_NONE, 0
            return self.PRI_TOWARD | (maxdist - dist), (1 if pair == pospair else -1)
        # (3) Check away from black
        # (3a) check both directions
        if pospc == self.PC_BLACK and negpc == self.PC_BLACK:
            posdist = self.L1Norm(self.agent, pospair)
            negdist = self.L1Norm(self.agent, negpair)
            if posdist == negdist:
                return self.PRI_NONE, 0
            return self.PRI_AWAY | (maxdist - min(posdist, negdist)), (-1 if posdist < negdist else 1)
        # (3b) check for singular pieces
        if (pospc, negpc) in [(self.PC_BLACK, self.PC_EMPTY), (self.PC_EMPTY, self.PC_BLACK)]:
            pair = (pospair if pospc == self.PC_BLACK else negpair)
            dist = self.L1Norm(self.agent, pair)
            return self.PRI_AWAY | (maxdist - dist), (-1 if pair == pospair else 1)
        return self.PRI_NONE, 0

    def __getitem__(self, pair):
        return self.columns[pair[0]][pair[1]]

    def __setitem__(self, pair, value):
        self.columns[pair[0]][pair[1]] = value
