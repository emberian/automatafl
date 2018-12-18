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
parser.add_argument('--dropout', dest='dropout', type=float, default=0.02, help='Drop this fraction of values betwen the internal layers to prevent overfit')
parser.add_argument('--memory', dest='memory', type=int, default=10000, help='Remember this many past moves for the learner')
parser.add_argument('--against', dest='against', help='Load this file as the adversary (instead of a random agent)')
parser.add_argument('--rand-rate', dest='rand_rate', type=float, default=0.02, help='Have the adversary move randomly at this rate')
parser.add_argument('--learn-rate', dest='learn_rate', type=float, default=0.1, help='Initial learning rate')
parser.add_argument('--layers', dest='layers', type=int, default=8, help='Use this many hidden layers')
parser.add_argument('--width', dest='width', type=int, default=128, help='Each hidden layer has this many neurons')
parser.add_argument('--update', dest='update', type=int, default=32, help='Update the target model with learned data after this many steps')
args = parser.parse_args()

plebs = [Plebeian(i) for i in range(1, 3)]
def setup_game():
    return Game(*plebs, setup=[
#        [2, 0, 0, 2, 0, 0, 2],
#        [0, 0, 1, 2, 1, 0, 0],
#        [1, 0, 0, 0, 0, 0, 1],
#        [2, 0, 0, 3, 0, 0, 2],
#        [1, 0, 0, 0, 0, 0, 1],
#        [0, 0, 1, 2, 1, 0, 0],
#        [2, 0, 0, 2, 0, 0, 2],
#    ], goals=[[(0, 0), (0, 6)], [(6, 0), (6, 6)]])
        [2, 0, 1, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 3, 0, 2],
        [0, 0, 0, 0, 0],
        [2, 0, 1, 0, 2],
    ], goals=[[(0, 0), (4, 0)], [(0, 4), (4, 4)]])

game = setup_game()

NUM_ACTIONS = game.NumActions()
NUM_STATES = len(game.StateVector(plebs[0]))

#print(NUM_ACTIONS)
#print(NUM_STATES)
#exit()

def make_net(primary):
    mdl = Sequential()
    mdl.add(Flatten(input_shape=(args.memory, NUM_STATES)))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(args.width, input_shape=(NUM_STATES,), activation='relu'))
    mdl.add(Dropout(args.dropout))
    if primary:
        for i in range(args.layers - 1):
            mdl.add(Dense(args.width, activation='relu', kernel_initializer='lecun_uniform'))
            mdl.add(Dropout(args.dropout))
    mdl.add(Dense(NUM_ACTIONS))
    return mdl

def make_agent(prim, load):
    nn = make_net(True)
    mem = SequentialMemory(limit=args.memory, window_length=args.memory)
    pol = BoltzmannQPolicy()
    dqn = DQNAgent(model=nn, nb_actions=NUM_ACTIONS, memory=mem, policy=pol, target_model_update=args.update)
    dqn.compile(Adam(lr=args.learn_rate), metrics=['mae'])
    if load:
        dqn.load_weights(load)
    return dqn

cur = make_agent(True, args.load)
if args.against:
    adv = make_agent(True, args.against)

steps = 0
class GameEnv(object):
    def reset(self):
        global game, steps
        game = setup_game()
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
        if args.against and random.random() > args.rand_rate:
            game.PoseAgentMove(plebs[1], adv.forward(game.StateVector(plebs[1])))
        else:
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

cur.fit(GameEnv(), nb_steps=args.steps, log_interval=args.update)
cur.save_weights(args.save, overwrite=True)
