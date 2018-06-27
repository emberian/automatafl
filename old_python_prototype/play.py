import argparse, json, asyncio, random

import numpy as np
from keras.models import Sequential
from keras.layers.core import Dense, Dropout
from keras.optimizers import RMSprop
import websockets

from model import Game, Plebeian, Board
import model

parser = argparse.ArgumentParser(description='Run a trained agent against the websocket server.')
parser.add_argument('load', help='Load weights from this file')
parser.add_argument('--train', dest='train', action='store_true', help='Also train against the other player (TODO)')
parser.add_argument('--player', dest='player', type=int, default=1, help='Be this player')
parser.add_argument('--name', dest='name', default='AI', help='Assume this name')
parser.add_argument('--uri', dest='uri', default='ws://localhost:8080/', help='Use this websocket URI')
parser.add_argument('--wait-time', dest='wait_time', type=float, default=0.2, help='Time spent waiting before assuming the last queued state response was received')
parser.add_argument('--dropout', dest='dropout', type=float, default=0.2, help='Network dropout (keep this ~the same or maybe less than what was used for learning)')
args = parser.parse_args()

# XXX Constants are from the game object which isn't created yet
def make_net():
    mdl = Sequential()
    mdl.add(Dense(1024, input_shape=(605,), activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1024, activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1024, activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(1024, activation='relu'))
    mdl.add(Dropout(args.dropout))
    mdl.add(Dense(5324))
    mdl.compile(loss='mse', optimizer=RMSprop())
    mdl.load_weights(args.load)
    return mdl

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
    while True:
        state = await get_latest_state(ws)
        if state is not None:
            return state

PC_CHAR = [' ', 'W', 'B', 'A']
def show_board(cols):
    width = len(cols[0])
    height = len(cols)
    for row in range(height - 1, -1, -1):
        for col in range(width):
            cell = cols[col][row]
            char = PC_CHAR[cell & 0xF]
            cfl = '!' if cell & Board.PC_F_CONFLICT else ' '
            goal = 'G' if cell & Board.PC_F_GOAL else ' '
            print(f'{cfl}{char}{goal} ', end='')
        print()

async def play_game(uri):
    nn = make_net()
    plebs = [Plebeian(i) for i in range(1, 3)]
    game = Game(*plebs)

    async with websockets.connect(uri) as ws:
        await ws.send(f'set_name {args.name}\n')
        await ws.send(f'be_p {args.player}\n')

        winner = None
        while winner is None:
            #game.board.columns = (await wait_until_new_state(ws))['columns']
            print('Beginning to consider move, current board state:')
            show_board(game.board.columns)
            act = np.argmax(nn.predict(np.array([game.StateVector(args.player)]), batch_size=1))
            mev = game.ActionToMove(None, act)
            print(f'Model is considering action {act}--from {mev.srcpair} to {mev.dstpair}')
            while True:
                await ws.send(f'move {mev.srcpair[0]} {mev.srcpair[1]} {mev.dstpair[0]} {mev.dstpair[1]}\n')
                while True:
                    pkt = json.loads(await ws.recv())
                    if 'kind' in pkt:
                        break
                    print('Ignoring', pkt)

                if pkt['kind'] == 'MOVE_INVALID':
                    print('Agent scheduled an invalid move; choosing a random one.')
                    mev = game.ActionToMove(None, random.randrange(game.NumActions()))
                elif pkt['kind'] == 'MOVE_ACK':
                    print('Move accepted')
                    break

            print('Waiting for TURN_OVER')
            while True:
                pkt = json.loads(await ws.recv())
                if pkt.get('kind') == 'TURN_OVER':
                    winner = pkt['winner']
                    break
                if pkt.get('msg') == 'state':
                    game.board.columns = pkt['columns']
                    print('state updated, now:')
                    show_board(game.board.columns)
                else:
                    print('Ignoring packet', pkt)

        print('Winner was', winner)

asyncio.get_event_loop().run_until_complete(play_game(args.uri))
