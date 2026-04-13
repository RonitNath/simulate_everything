"""
Adapts the Rust server's flat JSON observation into 2D numpy arrays
matching the interface expected by the strategic and graph agents.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass
class Observation:
    """Mirrors the fields the strategic agent reads from server observations."""

    armies: np.ndarray  # (H, W) int32
    owned_cells: np.ndarray  # (H, W) bool
    opponent_cells: np.ndarray  # (H, W) bool
    neutral_cells: np.ndarray  # (H, W) bool
    fog_cells: np.ndarray  # (H, W) bool
    structures_in_fog: np.ndarray  # (H, W) bool
    generals: np.ndarray  # (H, W) bool
    cities: np.ndarray  # (H, W) bool
    mountains: np.ndarray  # (H, W) bool
    owned_land_count: int
    owned_army_count: int
    opponent_land_count: int
    opponent_army_count: int
    timestep: int
    player: int
    width: int
    height: int


def parse_observation(msg: dict) -> Observation:
    """Convert a server observation JSON message into an Observation."""
    w = msg["width"]
    h = msg["height"]
    player = msg["player"]
    turn = msg["turn"]

    tiles_flat = msg["tiles"]
    armies_flat = msg["armies"]
    owners_flat = msg["owners"]
    visible_flat = msg["visible"]

    armies = np.array(armies_flat, dtype=np.int32).reshape(h, w)
    visible = np.array(visible_flat, dtype=bool).reshape(h, w)

    # Tile type masks
    mountains = np.zeros((h, w), dtype=bool)
    cities = np.zeros((h, w), dtype=bool)
    generals_arr = np.zeros((h, w), dtype=bool)

    for i, tile in enumerate(tiles_flat):
        r, c = divmod(i, w)
        if tile == "Mountain":
            mountains[r, c] = True
        elif tile == "City":
            cities[r, c] = True
        elif tile == "General":
            generals_arr[r, c] = True

    # Ownership masks
    owned = np.zeros((h, w), dtype=bool)
    opponent = np.zeros((h, w), dtype=bool)
    for i, owner in enumerate(owners_flat):
        r, c = divmod(i, w)
        if owner == player:
            owned[r, c] = True
        elif owner is not None:
            opponent[r, c] = True

    # Fog: not visible and not a known structure
    # Mountains and cities are visible even in fog on the Rust server,
    # but we still track "structures in fog" for pathfinding cost estimation.
    fog = ~visible & ~mountains & ~cities
    structures_in_fog = ~visible & (cities | mountains)

    # Neutral: visible, not owned, not opponent, not fog, not mountain
    neutral = visible & ~owned & ~opponent & ~mountains

    # Stats
    owned_land = int(msg["my_land"])
    owned_army = int(msg["my_armies"])

    # opponent_stats: list of [player_id, visible_land, global_armies]
    opp_stats = msg.get("opponent_stats", [])
    opp_land = 0
    opp_army = 0
    for stat in opp_stats:
        opp_land += stat[1]
        opp_army += stat[2]

    return Observation(
        armies=armies,
        owned_cells=owned,
        opponent_cells=opponent,
        neutral_cells=neutral,
        fog_cells=fog,
        structures_in_fog=structures_in_fog,
        generals=generals_arr,
        cities=cities,
        mountains=mountains,
        owned_land_count=owned_land,
        owned_army_count=owned_army,
        opponent_land_count=max(opp_land, 1),
        opponent_army_count=max(opp_army, 1),
        timestep=turn,
        player=player,
        width=w,
        height=h,
    )
