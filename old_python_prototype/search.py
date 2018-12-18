import multiprocessing, sys, random

import model

pool = None
def get_pool():
    global pool
    if pool is None:
        pool = multiprocessing.Pool()
    return pool

def search_d1_process_opponent(mxm, gm, maxpi, minpi, init):
    maxp, minp = gm.plebeians[maxpi], gm.plebeians[minpi]
    acts = gm.NumActions()
    maxmv = gm.ActionToMove(maxp, mxm)
    if maxmv.dstpair not in gm.board or (maxmv.dstpair == maxmv.srcpair):
        return None
    best = None

    for mnm in range(acts):
        minmv = gm.ActionToMove(minp, mnm)
        if minmv.dstpair not in gm.board or (minmv.dstpair == minmv.srcpair):
            continue

        gm.board = init.Copy()
        gm.locked.clear()
        gm.pending_moves.clear()

        gm.PoseAgentMove(maxp, mxm)
        gm.PoseAgentMove(minp, mnm)

        rwx, rwn = gm.RewardScalar(maxp), gm.RewardScalar(minp)
        loss = rwn - rwx

        minp.Events()
        maxp.Events()
        gm.GlobalEvents()

        if best is None or loss > best[0]:
            best = (loss, mxm, mnm)

    if best is not None:
        minmv = gm.ActionToMove(minp, best[2])
        score = best[0]
    else:
        minmv = model.Move(minp, (-1, -1), (-1, -1))
        score = '\x1b[1;31mNO BEST'
    print(f'\x1b[G{mxm:04d} / {acts:04d}: \x1b[1;36m{score} \x1b[32mme: {maxmv.srcpair} -> {maxmv.dstpair} \x1b[31madv: {minmv.srcpair} -> {minmv.dstpair}\x1b[m', end='')
    sys.stdout.flush()
    return best

def search_d1(gm, maxp):
    maxpi = 1 if maxp is gm.plebeians[0] else 0
    minpi = 0 if maxpi == 1 else 1

    init = gm.board.Copy()
    acts = gm.NumActions()
    best = None

    maxs = list(get_pool().starmap(search_d1_process_opponent, ((i, gm, maxpi, minpi, init) for i in range(acts))))
    for cand in maxs:
        if cand is None:
            continue
        if best is None or cand[0] < best[0]:
            best = cand
    best = random.choice([i for i in maxs if i is not None and i[0] == best[0]])

    gm.board = init.Copy()
    gm.locked.clear()
    return best

def search_d0(gm, maxp, rf=None):
    if rf is None:
        rf = model.Game.RewardScalar

    minp = gm.plebeians[0] if maxp is gm.plebeians[1] else gm.plebeians[1]

    init = gm.board.Copy()
    acts = gm.NumActions()
    best = None

    for act in range(acts):
        gm.board = init.Copy()
        gm.locked.clear()
        gm.pending_moves = {minp: ((-1, -1), (-1, -1))}
        gm.PoseAgentMove(maxp, act)
        rw = rf(gm, act, init)
        minp.Events()
        maxp.Events()
        gm.GlobalEvents()
        if best is None or rw > best[0]:
            best = (rw, act)

    return best
