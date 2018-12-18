import argparse
import numpy as np

from model import Board, Game, Plebeian

from keras.models import Sequential
from keras.layers import Dense, AlphaDropout, Dropout, Flatten, Reshape
from keras.optimizers import RMSprop, Adam

parser = argparse.ArgumentParser(description='See the reward space')
parser.add_argument('weights', help='Weight file to load')
parser.add_argument('--layers', dest='layers', type=int, default=8, help='Use this many hidden layers')
parser.add_argument('--width', dest='width', type=int, default=128, help='Each hidden layer has this many neurons')
parser.add_argument('--sec-width', dest='sec_width', type=int, help='Hidden layers after the first have this many neurons')
parser.add_argument('--dropout', dest='dropout', type=float, default=0.02, help='Drop this fraction of values betwen the internal layers to prevent overfit')
args = parser.parse_args()

if args.sec_width is None:
    args.sec_width = args.width

plebs = [Plebeian(i) for i in range(1, 3)]
def setup_game():
    return Game(*plebs, setup=[
        [2, 0, 1, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 0, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 1, 0, 2],
    ], goals=[[(0, 0), (4, 0)], [(0, 4), (4, 4)]])

game = setup_game()
setup = game.board.Copy()

NUM_ACTIONS = game.NumActions()
NUM_STATES = len(game.StateVector(plebs[0]))

def make_net(primary):
    mdl = Sequential()
    #mdl.add(Flatten(input_shape=(NUM_STATES, 1)))
    #mdl.add(Dropout(args.dropout))
    #mdl.add(Reshape((125,)))
    mdl.add(Dense(args.width, input_shape=(NUM_STATES + NUM_ACTIONS,), activation='relu', use_bias=False))
    mdl.add(Dropout(args.dropout))
    if primary:
        for i in range(args.layers - 1):
            mdl.add(Dense(args.sec_width, activation='relu', kernel_initializer='lecun_uniform', use_bias=False))
            mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1))
    return mdl

nn = make_net(True)
nn.compile(Adam(lr=0.0), loss='mse')
print('loading from', args.weights)
nn.load_weights(args.weights)

print('For agent positions:')

for row in range(setup.height-1, -1, -1):
    for col in range(setup.width):
        bd = setup.Copy()
        bd[col, row] = (bd[col, row] & (~0xF)) | bd.PC_AGENT
        game.board = bd
        goal = f'G{bd[col, row] >> 8}' if bd[col, row] & bd.PC_F_GOAL else ' '
        rw = game.board.RewardScalar(1)
        best = None
        for act in range(NUM_ACTIONS):
            sv = np.array(game.StateVector(plebs[0]))
            actv = np.zeros((NUM_ACTIONS,))
            actv[act] = 1
            q = nn.predict(np.concatenate((sv, actv)).reshape((1, NUM_ACTIONS + NUM_STATES)), batch_size=1)[0][0]
            if best is None or best[0] < q:
                best = (q, act)
        print(f'{best[0]:8.6f}/{game.ActionToMove(None, best[1])}/{rw:8.6f}{goal}', end='\t')
    print()

print('Initial state, per move:')
col, row = setup.width // 2, setup.height // 2
setup[col, row] = (setup[col, row] & (~0xF)) | setup.PC_AGENT
best = None

for act in range(NUM_ACTIONS):
    game.board = setup.Copy()
    game.locked.clear()
    game.pending_moves = {plebs[1]: ((-1, -1), (-1, -1))}
    game.PoseAgentMove(plebs[0], act)
    sv = np.array(game.StateVector(plebs[0]))
    actv = np.zeros((NUM_ACTIONS,))
    actv[act] = 1
    q = nn.predict(np.concatenate((sv, actv)).reshape((1, NUM_ACTIONS + NUM_STATES)), batch_size=1)[0][0]
    print(f'{game.ActionToMove(None, act)}: {q}', end='')
    if best is None or best[0] < q:
        print('(best so far)')
        best = (q, act)
    else:
        print()

print(f'Overall best was {game.ActionToMove(None, best[1])}')
