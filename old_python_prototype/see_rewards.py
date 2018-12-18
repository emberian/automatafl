import argparse

from model import Board

parser = argparse.ArgumentParser(description='See the reward space')
parser.add_argument('--size', '-n', default=11, type=int, help='On a this by this board')
args = parser.parse_args()

setup = Board(*[[0 for i in range(args.size)] for j in range(args.size)])
setup.SetPlayerGoal((0, 0), 1)
setup.SetPlayerGoal((args.size-1, 0), 1)
setup.SetPlayerGoal((0, args.size-1), 2)
setup.SetPlayerGoal((args.size-1, args.size-1), 2)

print('For player 1:\n')

for row in range(args.size-1, -1, -1):
    for col in range(args.size):
        bd = setup.Copy()
        bd[col, row] = (bd[col, row] & (~0xF)) | bd.PC_AGENT
        goal = f'G{bd[col, row] >> 8}' if bd[col, row] & bd.PC_F_GOAL else ' '
        print(f'{bd.RewardScalar(1): 8.6f}{goal}', end='\t')
    print()

print('For player 2:\n')

for row in range(args.size-1, -1, -1):
    for col in range(args.size):
        bd = setup.Copy()
        bd[col, row] = (bd[col, row] & (~0xF)) | bd.PC_AGENT
        goal = f'G{bd[col, row] >> 8}' if bd[col, row] & bd.PC_F_GOAL else ' '
        print(f'{bd.RewardScalar(2): 8.6f}{goal}', end='\t')
    print()
