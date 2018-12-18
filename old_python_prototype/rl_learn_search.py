import argparse, random
import numpy as np

from keras.models import Sequential
from keras.layers import Dense, AlphaDropout, Dropout, Flatten, Reshape
from keras.optimizers import RMSprop, Adam
from keras.initializers import RandomUniform

from model import Game, Board, Plebeian
import model, search

parser = argparse.ArgumentParser(description='Train a learning agent to play Automatafl.')
parser.add_argument('save', help='Save weights to this file')
parser.add_argument('-L', '--load', dest='load', help='Load these weights before training')
parser.add_argument('-s', '--steps', dest='steps', type=int, default=1000, help='Perform this many training steps')
parser.add_argument('-q', '--quiet', dest='quiet', action='store_true', help='Limited output')
parser.add_argument('--dropout', dest='dropout', type=float, default=0.02, help='Drop this fraction of values betwen the internal layers to prevent overfit')
parser.add_argument('--learn-rate', dest='learn_rate', type=float, default=0.2, help='Initial learning rate')
parser.add_argument('--epochs', dest='epochs', type=int, default=4, help='Number of training epochs')
parser.add_argument('--nonet-init', dest='nonet_init', type=float, default=1.0, help='Don\'t consult the net by this probability initially')
parser.add_argument('--nonet-decay', dest='nonet_decay', type=float, default=0.0001, help='Decrease non-consultation by this much per update')
parser.add_argument('--rand', dest='rand', type=float, default=0.999, help='How many of the "nonet" consultations to make random (as opposed to search)')
parser.add_argument('--gamma', dest='gamma', type=float, default=0.5, help='Discount factor')
parser.add_argument('--alpha', dest='alpha', type=float, default=1.0, help='(Q) learn rate')
parser.add_argument('--opp-rand', dest='opp_rand', type=float, default=0.999, help='Opponent plays randomly this much of the time')
parser.add_argument('--opp-search', dest='opp_search', default='search_d1', help='Opponent uses this searcher')
parser.add_argument('--search', dest='search', default='search_d1', help='Searcher function')
parser.add_argument('--layers', dest='layers', type=int, default=8, help='Use this many hidden layers')
parser.add_argument('--width', dest='width', type=int, default=128, help='Each hidden layer has this many neurons')
parser.add_argument('--sec-width', dest='sec_width', type=int, help='Hidden layers after the first have this many neurons')
parser.add_argument('--update', dest='update', type=int, default=10, help='Update the target model with learned data after this many steps (controls memory when reset is enabled)')
parser.add_argument('--no-reset', dest='no_reset', action='store_true', help='Do not reset the game after each update round (can cause a wedge)')
args = parser.parse_args()

if args.sec_width is None:
    args.sec_width = args.width

plebs = [Plebeian(i) for i in range(1, 3)]
def setup_game():
    return Game(*plebs, setup=[
        [2, 0, 1, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 3, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 1, 0, 2],
    ], goals=[[(0, 0), (4, 0)], [(0, 4), (4, 4)]])

game = setup_game()

NUM_ACTIONS = game.NumActions()
NUM_STATES = len(game.StateVector(plebs[0]))

def make_net(primary):
    mdl = Sequential()
    #mdl.add(Flatten(input_shape=(NUM_STATES, 1)))
    #mdl.add(Dropout(args.dropout))
    #mdl.add(Reshape((125,)))
    mdl.add(Dense(args.width, input_shape=(NUM_STATES + NUM_ACTIONS,), activation='relu', kernel_initializer=RandomUniform(-1, 1), use_bias=False))
    mdl.add(Dropout(args.dropout))
    if primary:
        for i in range(args.layers - 1):
            mdl.add(Dense(args.sec_width, activation='relu', kernel_initializer=RandomUniform(-1, 1), use_bias=False))
            mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1))
    return mdl

nn = make_net(True)
nn.compile(Adam(lr=args.learn_rate), loss='mse')
if args.load:
    print('loading from', args.load)
    nn.load_weights(args.load)

searcher = getattr(search, args.search)
oppsearcher = getattr(search, args.opp_search)
nonet = args.nonet_init
dc_powers = tuple(args.gamma ** i for i in range(args.update))

def consult_net(gm, act, init):
    sv = gm.StateVector(plebs[0])
    actv = np.zeros((NUM_ACTIONS,))
    actv[act] = 1
    return nn.predict(np.concatenate((np.array(sv), actv)).reshape((1, NUM_STATES + NUM_ACTIONS)), batch_size=1)

try:
    for step in range(args.steps):
        print('Step', step, 'nonet', nonet)
        hist = []

        for ply in range(args.update):
            if not args.quiet:
                print('Ply', ply)
                game.board.Show()
            
            if random.random() < nonet:
                if random.random() < args.rand:
                    if not args.quiet:
                        print('rand')
                    act = random.randrange(0, NUM_ACTIONS)
                else:
                    if not args.quiet:
                        print('search')
                    act = searcher(game, plebs[0])[1]
            else:
                if not args.quiet:
                    print('consult')
                act = search.search_d0(game, plebs[0], consult_net)[1]

            mv = game.ActionToMove(plebs[0], act)
            print('proposed:', mv)
            game.PoseAgentMove(plebs[0], act)

            if random.random() < args.opp_rand:
                if not args.quiet:
                    print('opprand')
                oppact = random.randrange(0, NUM_ACTIONS)
            else:
                if not args.quiet:
                    print('oppsearch')
                oppact = oppsearcher(game, plebs[1])[1]

            mv = game.ActionToMove(plebs[1], oppact)
            print('opponent:', mv)
            game.PoseAgentMove(plebs[1], oppact)

            winner = None
            for ev in game.GlobalEvents():
                if ev.__class__ is model.TurnOver and ev.winner is not None:
                    winner = ev.winner
                    print('game won by', winner)
                if ev.__class__ is model.Conflict:
                    print('conflict!')

            actv = np.zeros((NUM_ACTIONS,))
            actv[act] = 1
            hist.append((np.array(game.StateVector(plebs[0])), actv, game.RewardScalar(plebs[0])))

            if winner is not None:
                game = setup_game()

        print('training...')

        if not args.no_reset:
            game = setup_game()

        xs = np.array([np.concatenate((i[0], i[1])) for i in hist])
        old_ys = nn.predict(xs, batch_size=args.update)
        ys = np.array([(1 - args.alpha) * old_ys[i][0] + args.alpha * sum(hist[i+j][2] * dc_powers[j] for j in range(args.update - i)) for i in range(args.update)])

        nn.fit(xs, ys, epochs=args.epochs, batch_size=args.update, verbose=2)

        nonet -= args.nonet_decay
        if nonet < 0:
            nonet = 0
finally:
    print('Saving weights...')
    nn.save_weights(args.save)
    print('Done')
