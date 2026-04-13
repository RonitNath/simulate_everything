#!/usr/bin/env python3
"""
Example agent client for the Generals live server.

Usage:
    pip install websockets
    python agent.py --name MyBot --host localhost --port 3333

The agent connects to the server, joins the lobby, and plays random moves.
Modify the `decide()` function to implement your own strategy.
"""

import argparse
import asyncio
import json
import random
import websockets


def decide(obs: dict) -> list[dict]:
    """
    Given an observation, return a list of move orders.
    Each order is either "Pass" or {"Move": {"row": r, "col": c, "dir": d, "split": false}}.
    dir is one of: "Up", "Down", "Left", "Right".

    You can return as many orders as you want per turn.
    Orders from the same source cell should only appear once (later ones may fail).
    """
    width = obs["width"]
    height = obs["height"]
    player = obs["player"]
    tiles = obs["tiles"]
    armies = obs["armies"]
    owners = obs["owners"]

    moves = []
    directions = ["Up", "Down", "Left", "Right"]
    deltas = {"Up": (-1, 0), "Down": (1, 0), "Left": (0, -1), "Right": (0, 1)}

    for row in range(height):
        for col in range(width):
            idx = row * width + col
            if owners[idx] == player and armies[idx] > 1:
                # Try each direction
                valid_dirs = []
                for d in directions:
                    dr, dc = deltas[d]
                    nr, nc = row + dr, col + dc
                    if 0 <= nr < height and 0 <= nc < width:
                        nidx = nr * width + nc
                        if tiles[nidx] != "Mountain":
                            valid_dirs.append(d)
                if valid_dirs:
                    d = random.choice(valid_dirs)
                    moves.append({
                        "Move": {
                            "row": row,
                            "col": col,
                            "dir": d,
                            "split": False,
                        }
                    })

    # Send a random subset of moves (1-5).
    if not moves:
        return []
    k = min(len(moves), random.randint(1, 5))
    return random.sample(moves, k)


async def run(name: str, host: str, port: int):
    uri = f"ws://{host}:{port}/ws/agent"
    print(f"Connecting to {uri} as '{name}'...")

    async with websockets.connect(uri) as ws:
        # Join the lobby.
        await ws.send(json.dumps({"type": "join", "name": name}))

        while True:
            raw = await ws.recv()
            msg = json.loads(raw)

            match msg["type"]:
                case "lobby":
                    print(
                        f"Lobby: {msg['players_connected']}/{msg['players_needed']} "
                        f"(you are player {msg['slot']})"
                    )

                case "game_start":
                    print(
                        f"Game starting! Player {msg['player']} on "
                        f"{msg['width']}x{msg['height']} map, "
                        f"{msg['num_players']} players"
                    )

                case "observation":
                    actions = decide(msg)
                    await ws.send(json.dumps({"type": "actions", "actions": actions}))
                    if msg["turn"] % 50 == 0:
                        print(
                            f"  Turn {msg['turn']}: "
                            f"land={msg['my_land']}, armies={msg['my_armies']}"
                        )

                case "game_end":
                    winner = msg.get("winner")
                    print(
                        f"Game over after {msg['turns']} turns. "
                        f"Winner: {'player ' + str(winner) if winner is not None else 'draw'}"
                    )

                case "error":
                    print(f"Error: {msg['message']}")
                    break


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generals agent client")
    parser.add_argument("--name", default="RandomBot", help="Agent name")
    parser.add_argument("--host", default="localhost", help="Server host")
    parser.add_argument("--port", type=int, default=3333, help="Server port")
    args = parser.parse_args()

    asyncio.run(run(args.name, args.host, args.port))
