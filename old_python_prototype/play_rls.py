import argparse, json, asyncio, random
import numpy as np

import websockets

from keras.models import Sequential
from keras.layers import Dense, AlphaDropout, Dropout, Flatten, Reshape
from keras.optimizers import RMSprop, Adam

from model import Game, Plebeian, Board
import model, search

parser = argparse.ArgumentParser(description='Run a search algorithm against the websocket server.')
parser.add_argument('weights', help='Weights file to use')
parser.add_argument('--layers', dest='layers', type=int, default=8, help='Use this many hidden layers')
parser.add_argument('--width', dest='width', type=int, default=128, help='Each hidden layer has this many neurons')
parser.add_argument('--sec-width', dest='sec_width', type=int, help='Hidden layers after the first have this many neurons')
parser.add_argument('--dropout', dest='dropout', type=float, default=0.02, help='Drop this fraction of values betwen the internal layers to prevent overfit')
parser.add_argument('--player', dest='player', type=int, default=1, help='Be this player')
parser.add_argument('--name', dest='name', default='SearchAI', help='Assume this name')
parser.add_argument('--join', dest='join', help='Join this game after connecting')
parser.add_argument('--uri', dest='uri', default='ws://localhost:8080/', help='Use this websocket URI')
parser.add_argument('--wait-time', dest='wait_time', type=float, default=0.2, help='Time spent waiting before assuming the last queued state response was received')
args = parser.parse_args()

if args.sec_width is None:
    args.sec_width = args.width

async def get_latest_state(ws):
    state = None
    while True:
        try:
            msg = json.loads(await asyncio.wait_for(ws.recv(), args.wait_time))
            if msg.get('msg') == 'state':
                state = msg
            else:
                print('Ignoring packet', msg)
        except asyncio.TimeoutError:
            return state

async def wait_until_new_state(ws):
    print('Waiting for state')
    await ws.send('state\n')
    while True:
        state = await get_latest_state(ws)
        if state is not None:
            return state

#NUM_ACTIONS = game.NumActions()
#NUM_STATES = len(game.StateVector(plebs[0]))
NUM_STATES = 125

def make_net(primary):
    mdl = Sequential()
    #mdl.add(Flatten(input_shape=(NUM_STATES, 1)))
    #mdl.add(Dropout(args.dropout))
    #mdl.add(Reshape((125,)))
    mdl.add(Dense(args.width, input_shape=(NUM_STATES + NUM_ACTIONS,), activation='relu'))
    mdl.add(Dropout(args.dropout))
    if primary:
        for i in range(args.layers - 1):
            mdl.add(Dense(args.sec_width, activation='relu', kernel_initializer='lecun_uniform'))
            mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1))
    return mdl

nn = make_net(True)
nn.compile(Adam(lr=0.0), loss='mse')
print('loading from', args.weights)
nn.load_weights(args.weights)

def consult_net(gm, act, init):
    sv = gm.StateVector(gm.plebeians[0])
    actv = np.zeros((NUM_ACTIONS,))
    actv[act] = 1
    return nn.predict(np.concatenate((np.array(sv), actv)).reshape((1, NUM_STATES + NUM_ACTIONS)), batch_size=1)

async def play_game(uri):
    plebs = [Plebeian(i) for i in range(1, 3)]
    me = plebs[0] if args.player == 1 else plebs[1]
    game = Game(*plebs)

    async with websockets.connect(uri) as ws:
        await ws.send(f'set_name {args.name}\n')
        if args.join:
            await ws.send(f'join_game {args.join}\n')
        await ws.send(f'be_p {args.player}\n')

        winner = None
        while winner is None:
            state = (await wait_until_new_state(ws))
            game.board.columns = state['columns']
            game.board.width = state['width']
            game.board.height = state['height']
            print('Beginning to consider move, current board state:')
            game.board.Show()
            best = search.search_d0(game, me, consult_net)
            print(f'Best loss is {best}')
            act = best[1]
            mev = game.ActionToMove(None, act)
            print(f'Model is considering action {act}--from {mev.srcpair} to {mev.dstpair}')
            await ws.send(f'move {mev.srcpair[0]} {mev.srcpair[1]} {mev.dstpair[0]} {mev.dstpair[1]}\n')
            while True:
                while True:
                    pkt = json.loads(await ws.recv())
                    if 'kind' in pkt:
                        break
                    if pkt.get('msg') == 'state':
                        game.board.columns = pkt['columns']
                        print('State update:')
                        game.board.Show()
                    print('Ignoring', pkt)

                if pkt['kind'] == 'MOVE_INVALID':
                    print('Agent scheduled an invalid move; trying again.')
                    break
                elif pkt['kind'] == 'MOVE_ACK':
                    print('Move accepted:', pkt)
                elif pkt['kind'] == 'TURN_OVER':
                    winner = pkt['winner']
                    break
                elif pkt['kind'] == 'CONFLICT':
                    print('Conflict occurred')
                    break
                else:
                    print('Ignoring (kind)', pkt)

        print('Winner was', winner)

asyncio.get_event_loop().run_until_complete(play_game(args.uri))
