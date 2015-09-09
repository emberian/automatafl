'''
automatafl -- Automatafl
model -- Datamodel

Defines the main datamodel for automatafl.
'''

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
    def __init(self, pleb):
        self.pleb = pleb

class MoveInvalid(OEvent):
    OK                 = 0
    POS_OCCUPIED       = 2
    POS_OOB            = 3
    POS_CONFLICT       = 4
    POS_CANT_MOVE_THAT = 5

    def __init__(self, pleb, reason):
        assert(reason in [self.OK, self.ALREADY_MOVED,
                          self.POS_OOB, self.POS_CONFLICT])
        self.pleb = pleb
        self.reason = reason

class TurnOver(OEvent):
    pass

class Game():
    ST_INIT = 0
    ST_WAITING = 1
    ST_RESOLVING = 2
    # Note: The internal lists are columns!
    DEFAULT_SETUP = [
        [2,2,0,0,2,2,2,0,0,2,2],
        [0,0,0,1,2,2,2,1,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0],
        [1,1,0,0,0,0,0,0,0,1,1],
        [2,2,0,0,0,3,0,0,0,2,2],
        [1,1,0,0,0,0,0,0,0,1,1],
        [0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,1,2,2,2,1,0,0,0],
        [2,2,0,0,2,2,2,0,0,2,2],
    ]
    DEFAULT_GOALS = {
        2: [[(0, 0), (10, 0)], [(10, 0), (10, 10)]],
        4: [[(0, 0)], [(10, 0)], [(10, 10)], [(0, 10)]],
    }
    def __init__(self, *plebs, setup=None):
        if setup is None:
            setup = self.DEFAULT_SETUP
            assert len(plebs) in self.DEFAULT_GOALS.keys()
        self.pending_moves = {}
        self.plebeians = list(plebs)
        self.board = Board(*setup)
        if setup is self.DEFAULT_SETUP:
            for plidx, goals in enumerate(self.DEFAULT_GOALS[len(plebs)]):
                for goal in goals:
                    self.board.SetPlayerGoal(goal, plebs[plidx])
        self.state = self.ST_INIT
    def Handle(self, iev):
        return getattr(self, 'ev_'+str(type(iev)), self.UnknownEvent)(iev)
    def UnknownEvent(self, iev):
        raise ValueError('Unknown input event %r'%(repr(iev),))
    def ev_Move(self, iev):
        if (self.board[srcpair] & 0x0F) in (Board.PC_EMPTY, Board.PC_ASIAN):
            return MoveInvalid(pleb, MoveInvalid.POS_CANT_MOVE_THAT)
        elif (self.board[srcpair] & 0x10) == 0
        if self.board.CanMove(iev.srcpair, iev.dstpair):
            self.pending_moves[pleb] = (iev.srcpair, iev.dstpair)
            return MoveAck(pleb)
        else:
            if self.board[dst
            return MoveInvalid(pleb, MoveInvalid.POS_


class Plebeian():
    def __init__(self, id):
        self.id = id
    @staticmethod
    def ToID(obj):
        if isinstance(obj, Plebeian):
            return obj.id
        return obj

class Board():
    PC_EMPTY = 0x00
    PC_WHITE = 0x01
    PC_BLACK = 0x02
    PC_ASIAN = 0x03
    PC_F_CONFLICT = 0x10
    PC_F_GOAL = 0x20

    def __init__(self, *cols):
        self.columns = list(cols)
        self.width = len(cols)
        self.height = len(cols[0])
        assert all(len(i) == len(cols[0]) for i in cols[1:])
        self.king = None
        self._FindKing()
    def _FindKing(self):
        for colidx, col in enumerate(self.columns):
            for rowidx, row in enumerate(col):
                if (self[col, row] & 0x0F) == self.PC_ASIAN:
                    self.king = (col, row)
                    return self.king
    def SetPlayerGoal(self, pair, pleb):
        pleb = Plebeian.ToID(pleb)
        cell = self[pair]
        cell = (cell & 0xFF) | self.PC_F_GOAL | (pleb << 8)
        self[pair] = cell
    def CanMove(self, srcpair, dstpair):
        if (self[dstpair] & 0x1F) != self.PC_EMPTY:
            return False
    def Move(self, srcpair, dstpair):
        if self.CanMove(srcpair, dstpair):
            self[dstpair] = self[srcpair]
            self[srcpair] = (self[srcpair] & ~0x0F) | self.PC_EMPTY
            return True
        return False
    def InBoard(self, pair):
        return 0 <= pair[0] < self.width and 0 <= pair[1] < self.height
    def KingStep(self):
        self._FindKing()
        nearest = {}
        for axis in [(1, 0), (0, 1), (-1, 0), (0, -1)]:
            start = self.king
            while True:
                start[0] = start[0] + axis[0]
                start[1] = start[1] + axis[1]
                if not self.InBoard(start):
                    nearest[axis] = None
                    break
                if (self[start] & 0x0F) != self.PC_EMPTY:
                    nearest[axis] = start
                    break
        assert all((self[i] & 0x0F) != self.PC_ASIAN for i in nearest.itervalues())
        colpri, coldir = self._DoKing(nearest[(1, 0)], nearest[(-1, 0)])
        rowpri, rowdir = self._DoKing(nearest[(0, 1)], nearest[(0, -1)])
        colpair = self.king[0] + coldir, self.king[1]
        rowpair = self.king[0], self.king[1] + rowdir
        if colpri >= rowpri and self.InBoard(colpair):
            return self.king, colpair
        if self.InBoard(rowpair):
            return self.king, rowpair
        return self.king, self.king
    @staticmethod
    def L2Norm(p1, p2):
        return abs(p1[0] - p2[0]), abs(p2[1] - p2[1])

    PRI_UNBAL = 0x30000
    PRI_TOWARD = 0x20000
    PRI_AWAY = 0x10000
    PRI_NONE = 0

    def _DoKing(self, pospair, negpair):
        pospc = (self[pospair] & 0x0F if pospair is not None else self.PC_EMPTY)
        negpc = (self[negpair] & 0x0F if negpair is not None else self.PC_EMPTY)
        maxdist = max(self.width, self.height)
        # (1) Check unbalanced pairs
        if (pospc, negpc) in [(self.PC_WHITE, self.PC_BLACK), (self.PC_BLACK, self.PC_WHITE)]:
            whpair = (pospair if pospc == self.PC_WHITE else negpair)
            blpair = (pospair if pospc == self.PC_BLACK else negpair)
            whdist = self.L2Norm(self.king, white)
            bldist = self.L2Norm(self.king, black)
            if whdist <= 1:
                return self.PRI_NONE, 0
            return self.PRI_UNBAL | ((maxdist - whdist) << 8) | bldist, (1 if whpair == pospair else -1)
        # (2) Check toward white
        # (2a) check both directions
        if pospc == self.PC_WHITE and negpc == self.PC_WHITE:
            posdist = self.L2Norm(self.king, pospair)
            negdist = self.L2Norm(self.king, negpair)
            if posdist == negdist:
                return self.PRI_NONE, 0
            if 1 in (posdist, negdist):
                return self.PRI_NONE, 0
            return self.PRI_TOWARD | (maxdist - min(posdist, negdist)), (1 if posdist < negdist else -1)
        # (2b) check for singular pieces
        if (pospc, negpc) in [(self.PC_WHITE, self.PC_EMPTY), (self.PC_EMPTY, self.PC_WHITE)]:
            pair = (pospair if pospc == self.PC_WHITE else negpair)
            dist = self.L2Norm(self.king, pair)
            return self.PRI_TOWARD | (maxdist - dist), (1 if pair == pospair else -1)
        # (3) Check away from black
        # (3a) check both directions
        if pospc == self.PC_BLACK and negpc == self.PC_BLACK:
            posdist = self.L2Norm(self.king, pospair)
            negdist = self.L2Norm(self.king, negpair)
            if posdist == negdist:
                return self.PRI_NONE, 0
            return self.PRI_AWAY | (maxdist - min(posdist, negdist)), (-1 if posdist < negdist else 1)
        # (3b) check for singular pieces
        if (pospc, negpc) in [(self.PC_BLACK, self.PC_EMPTY), (self.PC_EMPTY, self.PC_BLACK)]:
            pair = (pospair if pospc == self.PC_BLACK else negpair)
            dist = self.L2Norm(self.king, pair)
            return self.PRI_AWAY | (maxdist - dist), (-1 if pair == pospair else 1)
        return self.PRI_NONE, 0
    def __getitem__(self, pair):
        return self.columns[pair[0]][pair[1]]
    def __setitem__(self, pair, value):
        self.columns[pair[0]][pair[1]] = value
