#!/usr/bin/env python3
"""
Graph search agent client for the Rust simulate_everything server.

Usage:
    pip install websockets numpy
    python main.py --name GraphBot --host localhost --port 3333

Options:
    --ascii         Print ASCII board every N turns (default: off)
    --ascii-every   Frequency of ASCII prints (default: 10)
    --verbose       Print debug info each turn
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys

import websockets

from agent import GraphSearchAgent
from observation import Observation, parse_observation


def render_ascii(obs: Observation, msg: dict) -> str:
    """Render the observation as an ASCII board matching the Rust server's format."""
    w = obs.width
    h = obs.height
    tiles = msg["tiles"]
    armies_flat = msg["armies"]
    owners_flat = msg["owners"]

    # Player labels
    player_labels = {}
    player_labels[obs.player] = chr(ord("a"))
    next_label = 1
    for owner in owners_flat:
        if owner is not None and owner != obs.player and owner not in player_labels:
            player_labels[owner] = chr(ord("a") + next_label)
            next_label += 1

    # Determine column width
    max_army = max(armies_flat) if armies_flat else 0
    col_width = max(4, len(str(max_army)) + 2)

    lines = []

    # Header
    stats_parts = []
    stats_parts.append(f"Turn {obs.timestep}")
    stats_parts.append(f"{player_labels.get(obs.player, '?')}: {obs.owned_land_count} land {obs.owned_army_count} army")
    for stat in msg.get("opponent_stats", []):
        pid, land, army = stat
        label = player_labels.get(pid, "?")
        stats_parts.append(f"{label}: {land} land {army} army")
    lines.append(" | ".join(stats_parts))

    # Column headers
    header = " " * (len(str(h - 1)) + 1)
    for c in range(w):
        header += str(c).rjust(col_width)
    lines.append(header)

    # Grid rows
    for r in range(h):
        row_label = str(r).rjust(len(str(h - 1)))
        row_str = row_label + " "
        for c in range(w):
            idx = r * w + c
            tile = tiles[idx]
            army = armies_flat[idx]
            owner = owners_flat[idx]

            if tile == "Mountain":
                cell = "####"
            elif not msg["visible"][idx]:
                cell = "????"
            elif owner is None:
                if tile == "City":
                    cell = f"c{army:>{col_width - 1}}"
                else:
                    cell = "...." if army == 0 else f"  {army:>{col_width - 2}}"
            else:
                label = player_labels.get(owner, "?")
                if tile == "General":
                    cell = f"{label.upper()}{army:>{col_width - 1}}"
                elif tile == "City":
                    cell = f"{label}~{army:>{col_width - 2}}"
                else:
                    cell = f"{label}{army:>{col_width - 1}}"

            row_str += cell.rjust(col_width)
        lines.append(row_str)

    return "\n".join(lines)


async def run(
    name: str,
    host: str,
    port: int,
    ascii_every: int,
    verbose: bool,
):
    agent = GraphSearchAgent(name=name)
    uri = f"ws://{host}:{port}/ws/agent"
    print(f"Connecting to {uri} as '{name}'...")

    while True:
        try:
            async with websockets.connect(uri) as ws:
                await ws.send(json.dumps({"type": "join", "name": name}))

                while True:
                    raw = await ws.recv()
                    msg = json.loads(raw)

                    match msg["type"]:
                        case "lobby":
                            print(
                                f"Lobby: {msg['players_connected']}/{msg['players_needed']} "
                                f"(slot {msg['slot']})"
                            )

                        case "game_start":
                            agent.reset()
                            print(
                                f"Game start: player {msg['player']} on "
                                f"{msg['width']}x{msg['height']} map, "
                                f"{msg['num_players']} players"
                            )

                        case "observation":
                            obs = parse_observation(msg)
                            actions = agent.decide(obs)
                            await ws.send(json.dumps({"type": "actions", "actions": actions}))

                            turn = obs.timestep
                            if ascii_every > 0 and turn % ascii_every == 0:
                                print()
                                print(render_ascii(obs, msg))
                                print(f"  Orders: {len(actions)} | Phase: {agent._phase} | Mode: {agent._mode}")
                                if agent._current_target:
                                    print(f"  Target: {agent._current_target} ({agent._target_kind})")
                                print()

                            if verbose and turn % 5 == 0:
                                print(
                                    f"  T{turn}: land={obs.owned_land_count} army={obs.owned_army_count} "
                                    f"phase={agent._phase} mode={agent._mode} orders={len(actions)}"
                                )

                        case "game_end":
                            winner = msg.get("winner")
                            print(
                                f"Game over after {msg['turns']} turns. "
                                f"Winner: {'player ' + str(winner) if winner is not None else 'draw'}"
                            )

                        case "error":
                            print(f"Error: {msg['message']}", file=sys.stderr)
                            break

        except websockets.exceptions.ConnectionClosed:
            print("Connection closed, reconnecting in 2s...")
            await asyncio.sleep(2)
        except ConnectionRefusedError:
            print("Connection refused, retrying in 5s...")
            await asyncio.sleep(5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Graph search agent for simulate_everything")
    parser.add_argument("--name", default="GraphBot", help="Agent name")
    parser.add_argument("--host", default="localhost", help="Server host")
    parser.add_argument("--port", type=int, default=3333, help="Server port")
    parser.add_argument("--ascii-every", type=int, default=0, help="Print ASCII every N turns (0=off)")
    parser.add_argument("--verbose", action="store_true", help="Print debug info")
    args = parser.parse_args()

    asyncio.run(run(args.name, args.host, args.port, args.ascii_every, args.verbose))
