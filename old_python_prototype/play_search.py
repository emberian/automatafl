import argparse, json, asyncio, random

import websockets

from model import Game, Plebeian, Board
import model, search

parser = argparse.ArgumentParser(description='Run a search algorithm against the websocket server.')
parser.add_argument('searcher', help='Searcher to use (function name)')
parser.add_argument('--player', dest='player', type=int, default=1, help='Be this player')
parser.add_argument('--name', dest='name', default='SearchAI', help='Assume this name')
parser.add_argument('--join', dest='join', help='Join this game after connecting')
parser.add_argument('--uri', dest='uri', default='ws://localhost:8080/', help='Use this websocket URI')
parser.add_argument('--wait-time', dest='wait_time', type=float, default=0.2, help='Time spent waiting before assuming the last queued state response was received')
args = parser.parse_args()

searchf = getattr(search, args.searcher)

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

PC_CHAR = [' ', 'W', 'B', 'A']
def show_board(cols):
    width = len(cols[0])
    height = len(cols)
    for row in range(height - 1, -1, -1):
        for col in range(width):
            cell = cols[col][row]
            char = PC_CHAR[cell & 0xF]
            cfl = '!' if cell & Board.PC_F_CONFLICT else ' '
            goal = f'G{cell>>8}' if cell & Board.PC_F_GOAL else '  '
            print(f'{cfl}{char}{goal} ', end='')
        print()

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
            show_board(game.board.columns)
            best = searchf(game, me)
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
                        show_board(game.board.columns)
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
