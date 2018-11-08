import argparse, random

import numpy as np
from keras.models import Sequential
from keras.layers import Dense, AlphaDropout, Dropout, Flatten
from keras.optimizers import RMSprop, Adam

from rl.agents.dqn import DQNAgent
from rl.policy import BoltzmannQPolicy
from rl.memory import SequentialMemory

from model import Game, Board, Plebeian
import model

parser = argparse.ArgumentParser(description='Train a learning agent to play Automatafl.')
parser.add_argument('save', help='Save weights to this file')
parser.add_argument('-L', '--load', dest='load', help='Load these weights before training')
parser.add_argument('-s', '--steps', dest='steps', type=int, default=100000, help='Perform this many training steps')
parser.add_argument('--dropout', dest='dropout', type=float, default=0.2, help='Drop this fraction of values betwen the internal layers to prevent overfit')
parser.add_argument('--memory', dest='memory', type=int, default=3, help='Remember this many past moves for the learner')
args = parser.parse_args()

plebs = [Plebeian(i) for i in range(1, 3)]
game = Game(*plebs)

NUM_ACTIONS = game.NumActions()
NUM_STATES = len(game.StateVector(plebs[0]))

def make_net(primary):
    mdl = Sequential()
    mdl.add(Flatten(input_shape=(args.memory, NUM_STATES)))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1024, input_shape=(NUM_STATES,), activation='relu'))
    mdl.add(Dropout(args.dropout))
    if primary:
        mdl.add(Dense(1024, activation='relu', kernel_initializer='lecun_uniform'))
        mdl.add(Dropout(args.dropout))
    mdl.add(Dense(NUM_ACTIONS))
    return mdl

nn = make_net(True)
mem = SequentialMemory(limit=100000, window_length=args.memory)
pol = BoltzmannQPolicy()
dqn = DQNAgent(model=nn, nb_actions=NUM_ACTIONS, memory=mem, policy=pol)
if args.load:
    dqn.load_weights(args.load)
dqn.compile(Adam(lr=0.1), metrics=['mae'])

steps = 0
class GameEnv(object):
    def reset(self):
        global game, steps
        game = Game(*plebs)
        steps = 0
        print('Game reset')
        return game.StateVector(plebs[0])

    def render(self, mode='human', close=False):
        pass

    def close(self):
        pass

    def step(self, act):
        global steps
        steps += 1

        game.PoseAgentMove(plebs[0], act)
        game.PoseAgentMove(plebs[1], random.randrange(0, NUM_ACTIONS))

        winner = None
        for ev in game.GlobalEvents():
            if ev.__class__ is model.TurnOver and ev.winner is not None:
                winner = ev.winner
                print(f'Game won on step {steps} by {winner}')
            if ev.__class__ is model.Conflict:
                print(f'Conflict on step {steps}')

        for pleb in plebs:
            pleb.Events()

        retval = (
            game.StateVector(plebs[0]),
            game.RewardScalar(plebs[0]),
            winner is not None,
            {},
        )

        return retval

dqn.fit(GameEnv(), nb_steps=args.steps)
dqn.save_weights(args.save, overwrite=True)
