import argparse, random

import numpy as np
from keras.models import Sequential
from keras.layers.core import Dense, Dropout
from keras.optimizers import RMSprop

from model import Game, Board, Plebeian
import model

parser = argparse.ArgumentParser(description='Train a learning agent to play Automatafl.')
parser.add_argument('save', help='Save weights to this file')
parser.add_argument('-L', '--load', dest='load', help='Load these weights before training')
parser.add_argument('--no-rand', dest='no_rand', action='store_true', help='Also load the same weights into the opponents')
parser.add_argument('-s', '--steps', dest='steps', type=int, default=100000, help='Perform this many training steps')
parser.add_argument('--dropout', dest='dropout', type=float, default=0.2, help='Drop this fraction of values betwen the internal layers to prevent overfit')
parser.add_argument('--rand-chance', dest='rand_chance', type=float, default=0.9, help='Choose a uniform-random action initially this fraction of the steps (for bootstrapping observations)')
parser.add_argument('--rand-decay', dest='rand_decay', type=float, default=0.00001, help='Subtract this value from --rand-chance at each frame')
parser.add_argument('--observations', dest='observations', type=int, default=1024, help='Number of history elements to build before beginning a batch training (must be > --batch-size)')
parser.add_argument('--batch-size', dest='batch_size', type=int, default=64, help='Number of sampled history elements to input into training batch')
parser.add_argument('--q-learn-rate', dest='q_learn_rate', type=float, default=0.9, help='Amount to weight learned new Q values (helps forget poor, coinciental choices')
parser.add_argument('--rms-learn-rate', dest='rms_learn_rate', type=float, default=0.95, help='LR parameter to RMSprop optimizer')
parser.add_argument('--rms-decay', dest='rms_decay', type=float, default=0.001, help='Decary parameter to RMSprop optimizer')
args = parser.parse_args()

plebs = [Plebeian(i) for i in range(1, 3)]
game = Game(*plebs)

NUM_ACTIONS = game.NumActions()
NUM_STATES = len(game.StateVector(plebs[0]))

def make_net(primary):
    mdl = Sequential()
    mdl.add(Dense(1024, input_shape=(NUM_STATES,), activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(8192, activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(8192, activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(NUM_ACTIONS))
    mdl.compile(loss='mse', optimizer=RMSprop(lr=args.rms_learn_rate, decay=args.rms_decay))
    if args.load and (primary or args.no_rand):
        mdl.load_weights(args.load)
    return mdl

for pleb in plebs:
    pleb.nn = make_net(pleb.id == 1)
    pleb.history = []  # (state, action, reward) -- the next state is appended when batch processing

print('Performing SARSA training...')
rand_chance = args.rand_chance

try:
    for step in range(args.steps):
        if step % 1024 == 0:
            print(f'[Step {step} rand_chance {rand_chance}]')

        for pleb in plebs:
            state = np.array([game.StateVector(pleb)])
            pleb.state = state
            if random.random() < rand_chance:
                act = random.randrange(NUM_ACTIONS)
            else:
                act = np.argmax(pleb.nn.predict(state, batch_size=1))
            pleb.act = act
            game.PoseAgentMove(pleb, act)

        rand_chance -= args.rand_decay

        winner = None
        for ev in game.GlobalEvents():
            if isinstance(ev, model.TurnOver):
                if ev.winner is not None:
                    print(f'Game won by {ev.winner} on step {step}')
                    winner = ev.winner
                break
            if isinstance(ev, model.Conflict):
                print(f'Conflict encountered on step {step}')
                break
        else:
            raise RuntimeError('Game didn\'t advance state after both moves were proposed--bug.')
        
        for pleb in plebs:
            pleb.Events()  # Dump queue
            pleb.history.append((pleb.state, pleb.act, game.RewardScalar(pleb), winner is not None))

        if winner is not None:
            # Reset the game
            game = Game(*plebs)

        if step % args.observations == 0 and step > 0:
            print(f'Training at step {step}:')

            for pleb in plebs:
                print(f'For pleb {pleb.id}...', end='')
                batch = [old + (new[0],) for old, new in zip(pleb.history, pleb.history[1:])]  # SARS batch
                sample = random.sample(batch, args.batch_size)

                # Generate a batch matrix
                len_sample = len(sample)  # Should ideally be args.batch_size, but :shrug:
                prev_state = np.zeros(shape=(len_sample, NUM_STATES))
                actions = np.zeros(shape=(len_sample,), dtype=np.int16)
                rewards = np.zeros(shape=(len_sample,))
                terminal = np.zeros(shape=(len_sample,), dtype=np.bool)
                next_state = np.zeros(shape=(len_sample, NUM_STATES))

                for i, ent in enumerate(sample):
                    p_s, a, r, t, n_s = ent
                    prev_state[i, :] = p_s[...]
                    actions[i] = a
                    rewards[i] = r
                    terminal[i] = t
                    next_state[i, :] = n_s[...]

                # Compute Q values (action desirability as a function of state/choice) for before/after circumstances
                prev_q = pleb.nn.predict(prev_state, batch_size=len_sample)
                next_q = pleb.nn.predict(next_state, batch_size=len_sample)

                # Use next_q to determine the currently-apparent best action
                max_next_q = np.max(next_q, axis=1)

                term_episodes = np.where(terminal)[0]
                nonterm_episodes = np.where(terminal == False)[0]  # Yes, it has to be phrased this way.

                # Starting with our previous assumptions...
                new_q = prev_q  # Don't care about clobbering this, copy the reference
                # ...update the Q function to match our observations.
                new_q[nonterm_episodes, actions[nonterm_episodes]] = rewards[nonterm_episodes] + args.q_learn_rate * max_next_q[nonterm_episodes]
                new_q[term_episodes, actions[term_episodes]] = rewards[term_episodes]

                # And actually train the model on this batch sample.
                hist = pleb.nn.fit(prev_state, new_q, batch_size=len_sample, epochs=1, verbose=0)
                print(f'complete, history: {hist.history}')

                # So as not to explode memory, clear the entire history.
                del pleb.history[:]

finally:
    plebs[0].nn.save_weights(args.save)
    print('States saved.')
