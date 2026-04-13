"""
Graph-search reference agent for the Rust simulate_everything server.

Key characteristics of this implementation:
- Multiple orders per turn (no cap) instead of one action
- Army growth: structures +1/turn, land +1 with 10% probability (not 50-turn cycles)
- Pure numpy implementation
- WebSocket JSON protocol with string directions
- Income timing removed (no 50-turn income cycle)
"""

from __future__ import annotations

import heapq
import random
from collections import deque
from dataclasses import dataclass

import numpy as np

from observation import Observation

# Direction offsets: 0=UP, 1=DOWN, 2=LEFT, 3=RIGHT
_DR = np.array([-1, 1, 0, 0], dtype=np.int32)
_DC = np.array([0, 0, -1, 1], dtype=np.int32)
_REVERSE_DIR = np.array([1, 0, 3, 2], dtype=np.int32)
_DIR_NAMES = ["Up", "Down", "Left", "Right"]

_INF = 10**9


@dataclass(frozen=True)
class MoveFeatures:
    row: int
    col: int
    direction: int  # 0=UP, 1=DOWN, 2=LEFT, 3=RIGHT
    dest_row: int
    dest_col: int
    source_army: int
    dest_army: int
    moving_army: int
    can_capture: bool
    dest_is_owned: bool
    dest_is_opponent: bool
    dest_is_neutral: bool
    dest_is_city: bool
    dest_is_general: bool
    dest_is_fog: bool
    dest_is_structure_fog: bool


class GraphSearchAgent:
    """
    Multi-order graph-search agent for the Rust simulate_everything server.

    Returns a ranked list of moves per turn (exploiting unlimited orders).
    Uses BFS/Dijkstra pathfinding with strategic target selection,
    phase/mode inference, and punish-window tactical interrupts.
    """

    def __init__(self, name: str = "GraphSearch"):
        self.name = name
        self.reset()

    def reset(self):
        # Memory
        self._seen: np.ndarray | None = None
        self._last_seen_opponent: np.ndarray | None = None
        self._last_seen_generals: np.ndarray | None = None
        self._last_seen_cities: np.ndarray | None = None
        self._own_general: tuple[int, int] | None = None
        self._enemy_general_estimate: tuple[int, int] | None = None

        # Strategy
        self._current_target: tuple[int, int] | None = None
        self._target_kind: str | None = None
        self._target_commitment: int = 0
        self._phase: str = "opening"
        self._mode: str = "even"
        self._mode_metrics: dict = {}

        # Punish
        self._punish_target: tuple[int, int] | None = None
        self._punish_kind: str | None = None
        self._punish_reason: str | None = None
        self._punish_strength: float = 0.0
        self._punish_window: int = 0

        # Graph search state
        self._distance_map: np.ndarray | None = None
        self._cost_map: np.ndarray | None = None
        self._flow_direction: np.ndarray | None = None
        self._gather_map: np.ndarray | None = None
        self._gather_direction: np.ndarray | None = None
        self._plan_target: tuple[int, int] | None = None
        self._plan_gather_point: tuple[int, int] | None = None
        self._plan_turn: int = -_INF
        self._plan_mode: str = "expand"
        self._recompute_interval: int = 3

        # Move history (for anti-loop)
        self._recent_choices: deque = deque(maxlen=8)
        self._recent_action_kinds: deque = deque(maxlen=8)
        # Track which cells we already issued orders for this turn
        self._turn_ordered_sources: set[tuple[int, int]] = set()

    # ------------------------------------------------------------------
    # Public interface
    # ------------------------------------------------------------------

    def decide(self, obs: Observation) -> list[dict]:
        """Return a list of move orders for this turn (JSON-ready dicts)."""
        self._turn_ordered_sources = set()

        # State updates
        self._update_memory(obs)
        self._phase = self._infer_phase(obs)
        self._mode = self._infer_mode(obs)
        self._update_punish_window(obs)
        self._update_strategy_target(obs)

        # Recompute pathfinding maps when needed
        timestep = obs.timestep
        target_changed = self._current_target != self._plan_target
        should_recompute = (
            target_changed
            or self._distance_map is None
            or (timestep - self._plan_turn) >= self._recompute_interval
        )
        if should_recompute:
            self._recompute_maps(obs)

        self._plan_mode = self._select_plan_mode(obs)

        # Extract all valid moves
        moves = self._extract_moves(obs)
        if not moves:
            return []

        # Score all moves
        scores = self._score_all_moves(obs, moves)

        # Select multiple orders: pick best move per source cell
        # Sort by score descending, greedily pick non-conflicting sources
        order = np.argsort(scores)[::-1]
        orders: list[dict] = []

        # Budget: scale with territory but cap to focus on impactful moves.
        # Trivial army=2 reinforcements add noise without strategic value.
        max_orders = max(5, min(50, obs.owned_land_count // 4))

        for idx in order:
            if len(orders) >= max_orders:
                break

            move = moves[idx]
            src = (move.row, move.col)
            if src in self._turn_ordered_sources:
                continue

            score = scores[idx]
            # Skip low-scoring moves once we have a reasonable set
            if score < 0.0 and len(orders) >= 3:
                break

            split = self._choose_split(obs, move)
            kind = self._classify_move(move)
            self._remember_choice(move, kind)
            self._turn_ordered_sources.add(src)

            orders.append({
                "Move": {
                    "row": move.row,
                    "col": move.col,
                    "dir": _DIR_NAMES[move.direction],
                    "split": split,
                }
            })

        return orders

    # ------------------------------------------------------------------
    # Scoring pipeline
    # ------------------------------------------------------------------

    def _score_all_moves(self, obs: Observation, moves: list[MoveFeatures]) -> np.ndarray:
        n = len(moves)
        base = np.array([self._score_move(obs, m) for m in moves], dtype=np.float32)
        phase = np.array([self._phase_adjustment(obs, m) for m in moves], dtype=np.float32)
        mode = np.array([self._mode_adjustment(obs, m) for m in moves], dtype=np.float32)
        punish = np.array([self._punish_adjustment(obs, m) for m in moves], dtype=np.float32)
        cont = np.array([self._continuation_value(obs, m) for m in moves], dtype=np.float32)
        adj = np.array([self._score_adjustment(obs, m) for m in moves], dtype=np.float32)
        noise = np.random.uniform(-0.15, 0.15, size=n).astype(np.float32)
        return base + phase + mode + punish + cont + adj + noise

    def _score_move(self, obs: Observation, move: MoveFeatures) -> float:
        score = float(move.source_army) * 0.1

        if move.dest_is_general and move.can_capture:
            return 1e6

        # Path alignment
        if self._distance_map is not None:
            source_dist = int(self._distance_map[move.row, move.col])
            dest_dist = int(self._distance_map[move.dest_row, move.dest_col])
            if source_dist < _INF and dest_dist < _INF:
                path_progress = source_dist - dest_dist
                score += 15.0 * path_progress
                if self._flow_direction is not None:
                    optimal_dir = int(self._flow_direction[move.row, move.col])
                    if optimal_dir >= 0 and move.direction == optimal_dir:
                        score += 10.0

        # Cost feasibility
        if self._cost_map is not None and self._plan_mode in ("attack", "gather"):
            cost_here = int(self._cost_map[move.row, move.col])
            if cost_here < _INF:
                if move.moving_army > cost_here * 0.5:
                    score += 20.0
                elif move.moving_army < cost_here * 0.2 and not move.dest_is_owned:
                    score -= 10.0

        # Gathering
        if self._plan_mode == "gather" and self._gather_map is not None and move.dest_is_owned:
            src_gather = int(self._gather_map[move.row, move.col])
            dst_gather = int(self._gather_map[move.dest_row, move.dest_col])
            if src_gather < _INF and dst_gather < _INF:
                gather_progress = src_gather - dst_gather
                score += 12.0 * gather_progress + 8.0
                score += move.source_army * 0.3

        # Tactical bonuses
        if move.dest_is_opponent and move.can_capture:
            score += 60.0
        if move.dest_is_city and move.can_capture:
            # Cities are more valuable in the new ruleset (+1/turn instead of every 2)
            score += 65.0
        if move.dest_is_neutral and move.can_capture:
            score += 15.0
        if move.dest_is_fog:
            score += 8.0
        if move.dest_is_owned and self._plan_mode != "gather":
            score -= 5.0

        # Defense mode
        if self._plan_mode == "defend" and self._own_general is not None:
            home_dist = self._distance_from_own_general(move)
            if home_dist <= 4:
                score += 15.0 - home_dist * 2.0

        return score

    def _choose_split(self, obs: Observation, move: MoveFeatures) -> bool:
        if self._plan_mode == "gather" and move.dest_is_owned and move.source_army >= 6:
            return True
        if move.dest_is_owned and move.source_army >= 10:
            return True
        return False

    # ------------------------------------------------------------------
    # Phase / mode inference — adapted for new army gen rules
    # ------------------------------------------------------------------

    def _material_ratio(self, obs: Observation) -> float:
        mine = max(float(obs.owned_army_count), 1.0)
        opp = max(float(obs.opponent_army_count), 1.0)
        return mine / opp

    def _infer_phase(self, obs: Observation) -> str:
        timestep = obs.timestep
        ratio = self._material_ratio(obs)
        enemy_visible = bool(np.any(obs.opponent_cells))

        if ratio < 0.8:
            return "defense"
        if self._enemy_general_estimate is not None and ratio > 1.2 and enemy_visible:
            return "kill"
        # Shorter opening phase since armies grow faster in new rules
        if timestep < 40 and obs.owned_land_count < 15:
            return "opening"
        if ratio > 1.1:
            return "consolidation"
        if enemy_visible or timestep >= 40:
            return "pressure"
        return "expansion"

    def _infer_mode(self, obs: Observation) -> str:
        army_ratio = self._material_ratio(obs)
        land_ratio = max(float(obs.owned_land_count), 1.0) / max(float(obs.opponent_land_count), 1.0)

        if army_ratio >= 1.3 or (army_ratio >= 1.15 and land_ratio >= 1.1):
            mode = "ahead"
        elif army_ratio <= 0.78 or (army_ratio <= 0.92 and land_ratio <= 0.9):
            mode = "behind"
        else:
            mode = "even"

        self._mode_metrics = {
            "army_ratio": round(army_ratio, 3),
            "land_ratio": round(land_ratio, 3),
            "owned_army": obs.owned_army_count,
            "opponent_army": obs.opponent_army_count,
            "owned_land": obs.owned_land_count,
            "opponent_land": obs.opponent_land_count,
        }
        return mode

    # ------------------------------------------------------------------
    # Memory
    # ------------------------------------------------------------------

    def _update_memory(self, obs: Observation) -> None:
        h, w = obs.height, obs.width
        visible = ~(obs.fog_cells | obs.structures_in_fog)

        if self._seen is None:
            self._seen = np.zeros((h, w), dtype=bool)
            self._last_seen_opponent = np.zeros((h, w), dtype=bool)
            self._last_seen_generals = np.zeros((h, w), dtype=bool)
            self._last_seen_cities = np.zeros((h, w), dtype=bool)

        self._seen |= visible
        self._last_seen_opponent = np.where(obs.opponent_cells, True, self._last_seen_opponent)
        visible_enemy_generals = obs.generals & obs.opponent_cells
        self._last_seen_generals = visible_enemy_generals
        self._last_seen_cities = np.where(obs.cities, True, self._last_seen_cities)

        own_generals = np.argwhere(obs.generals & obs.owned_cells)
        if len(own_generals):
            self._own_general = (int(own_generals[0][0]), int(own_generals[0][1]))

        enemy_generals = np.argwhere(obs.generals & obs.opponent_cells)
        if len(enemy_generals):
            self._enemy_general_estimate = (int(enemy_generals[0][0]), int(enemy_generals[0][1]))
            return

        last_seen_enemy = np.argwhere(self._last_seen_opponent)
        if len(last_seen_enemy):
            avg = np.mean(last_seen_enemy, axis=0)
            self._enemy_general_estimate = (int(avg[0]), int(avg[1]))
            return

        fog_structures = np.argwhere(obs.structures_in_fog)
        if len(fog_structures):
            if self._own_general is not None:
                distances = np.sum(np.abs(fog_structures - np.asarray(self._own_general)), axis=1)
                self._enemy_general_estimate = tuple(map(int, fog_structures[int(np.argmax(distances))]))
            else:
                self._enemy_general_estimate = tuple(map(int, fog_structures[0]))
            return

        unseen = np.argwhere(~self._seen)
        if len(unseen):
            if self._own_general is not None:
                distances = np.sum(np.abs(unseen - np.asarray(self._own_general)), axis=1)
                self._enemy_general_estimate = tuple(map(int, unseen[int(np.argmax(distances))]))
            else:
                self._enemy_general_estimate = tuple(map(int, unseen[0]))

    # ------------------------------------------------------------------
    # Move extraction
    # ------------------------------------------------------------------

    def _extract_moves(self, obs: Observation) -> list[MoveFeatures]:
        armies = obs.armies
        owned = obs.owned_cells
        mountains = obs.mountains
        opp = obs.opponent_cells
        neutral = obs.neutral_cells
        cities = obs.cities
        generals = obs.generals
        fog = obs.fog_cells
        structures_fog = obs.structures_in_fog
        h, w = obs.height, obs.width

        moves: list[MoveFeatures] = []
        for r in range(h):
            for c in range(w):
                if not owned[r, c] or armies[r, c] <= 1:
                    continue
                for d in range(4):
                    nr = r + _DR[d]
                    nc = c + _DC[d]
                    if nr < 0 or nr >= h or nc < 0 or nc >= w:
                        continue
                    if mountains[nr, nc]:
                        continue

                    source_army = int(armies[r, c])
                    dest_army = int(armies[nr, nc])
                    moving_army = source_army - 1

                    moves.append(MoveFeatures(
                        row=r,
                        col=c,
                        direction=d,
                        dest_row=nr,
                        dest_col=nc,
                        source_army=source_army,
                        dest_army=dest_army,
                        moving_army=moving_army,
                        can_capture=moving_army > dest_army,
                        dest_is_owned=bool(owned[nr, nc]),
                        dest_is_opponent=bool(opp[nr, nc]),
                        dest_is_neutral=bool(neutral[nr, nc]),
                        dest_is_city=bool(cities[nr, nc]),
                        dest_is_general=bool(generals[nr, nc]),
                        dest_is_fog=bool(fog[nr, nc]),
                        dest_is_structure_fog=bool(structures_fog[nr, nc]),
                    ))
        return moves

    # ------------------------------------------------------------------
    # Pathfinding
    # ------------------------------------------------------------------

    @staticmethod
    def _bfs_from(
        target: tuple[int, int], passable: np.ndarray, H: int, W: int
    ) -> tuple[np.ndarray, np.ndarray]:
        dist = np.full((H, W), _INF, dtype=np.int32)
        flow = np.full((H, W), -1, dtype=np.int32)
        tr, tc = target
        if not (0 <= tr < H and 0 <= tc < W and passable[tr, tc]):
            return dist, flow
        dist[tr, tc] = 0
        queue: deque[tuple[int, int]] = deque()
        queue.append((tr, tc))
        while queue:
            r, c = queue.popleft()
            d = dist[r, c] + 1
            for direction in range(4):
                nr = r + _DR[direction]
                nc = c + _DC[direction]
                if 0 <= nr < H and 0 <= nc < W and passable[nr, nc] and dist[nr, nc] > d:
                    dist[nr, nc] = d
                    flow[nr, nc] = _REVERSE_DIR[direction]
                    queue.append((nr, nc))
        return dist, flow

    @staticmethod
    def _dijkstra_cost_from(
        target: tuple[int, int],
        passable: np.ndarray,
        armies: np.ndarray,
        owned: np.ndarray,
        opponent: np.ndarray,
        fog: np.ndarray,
        structures_fog: np.ndarray,
        timestep: int,
        H: int,
        W: int,
    ) -> np.ndarray:
        def cell_cost(r: int, c: int) -> int:
            if owned[r, c]:
                return 0
            a = int(armies[r, c])
            if opponent[r, c]:
                return a + 1
            if structures_fog[r, c]:
                # In new rules, cities grow every turn, so fog structures
                # accumulate armies faster
                return max(25, 12 + timestep // 20)
            if fog[r, c]:
                return max(1, timestep // 50)
            return a + 1 if a > 0 else 1

        cost = np.full((H, W), _INF, dtype=np.int64)
        tr, tc = target
        if not (0 <= tr < H and 0 <= tc < W and passable[tr, tc]):
            return cost
        cost[tr, tc] = cell_cost(tr, tc)
        heap: list[tuple[int, int, int]] = [(int(cost[tr, tc]), tr, tc)]
        while heap:
            c_val, r, c = heapq.heappop(heap)
            if c_val > cost[r, c]:
                continue
            for direction in range(4):
                nr = r + _DR[direction]
                nc = c + _DC[direction]
                if 0 <= nr < H and 0 <= nc < W and passable[nr, nc]:
                    new_cost = c_val + cell_cost(nr, nc)
                    if new_cost < cost[nr, nc]:
                        cost[nr, nc] = new_cost
                        heapq.heappush(heap, (int(new_cost), nr, nc))
        return cost

    @staticmethod
    def _bfs_from_owned(
        gather_point: tuple[int, int],
        owned: np.ndarray,
        passable: np.ndarray,
        H: int,
        W: int,
    ) -> tuple[np.ndarray, np.ndarray]:
        dist = np.full((H, W), _INF, dtype=np.int32)
        flow = np.full((H, W), -1, dtype=np.int32)
        gr, gc = gather_point
        if not (0 <= gr < H and 0 <= gc < W and owned[gr, gc] and passable[gr, gc]):
            return dist, flow
        dist[gr, gc] = 0
        queue: deque[tuple[int, int]] = deque()
        queue.append((gr, gc))
        while queue:
            r, c = queue.popleft()
            d = dist[r, c] + 1
            for direction in range(4):
                nr = r + _DR[direction]
                nc = c + _DC[direction]
                if (
                    0 <= nr < H
                    and 0 <= nc < W
                    and passable[nr, nc]
                    and owned[nr, nc]
                    and dist[nr, nc] > d
                ):
                    dist[nr, nc] = d
                    flow[nr, nc] = _REVERSE_DIR[direction]
                    queue.append((nr, nc))
        return dist, flow

    # ------------------------------------------------------------------
    # Planning helpers
    # ------------------------------------------------------------------

    def _select_gather_point(self, obs: Observation) -> tuple[int, int] | None:
        if self._distance_map is None or self._current_target is None:
            return self._own_general

        owned = obs.owned_cells
        armies = obs.armies
        H, W = obs.height, obs.width
        owned_positions = np.argwhere(owned)
        if len(owned_positions) == 0:
            return self._own_general

        best_score = -_INF
        best_pos: tuple[int, int] | None = None
        for pos in owned_positions:
            r, c = int(pos[0]), int(pos[1])
            dist = self._distance_map[r, c]
            if dist >= _INF:
                continue
            r_lo, r_hi = max(0, r - 1), min(H, r + 2)
            c_lo, c_hi = max(0, c - 1), min(W, c + 2)
            local_army = int(np.sum(armies[r_lo:r_hi, c_lo:c_hi] * owned[r_lo:r_hi, c_lo:c_hi]))
            score = local_army * 0.5 - dist * 2.0
            if score > best_score:
                best_score = score
                best_pos = (r, c)
        return best_pos if best_pos is not None else self._own_general

    def _select_plan_mode(self, obs: Observation) -> str:
        if self._phase == "defense":
            return "defend"

        if self._enemy_general_estimate is not None and self._cost_map is not None:
            owned = obs.owned_cells
            armies = obs.armies
            owned_armies = armies * owned
            if owned_armies.size > 0:
                best_idx = np.argmax(owned_armies)
                best_r, best_c = divmod(int(best_idx), obs.width)
                best_army = int(armies[best_r, best_c])
                cost = int(self._cost_map[best_r, best_c])
                if cost < _INF and best_army > cost * 1.3:
                    return "attack"

        if self._mode == "ahead" and obs.owned_army_count > 50:
            return "gather"

        return "expand"

    def _recompute_maps(self, obs: Observation) -> None:
        mountains = obs.mountains
        passable = ~mountains
        armies = obs.armies
        owned = obs.owned_cells
        opponent = obs.opponent_cells
        fog = obs.fog_cells
        structures_fog = obs.structures_in_fog
        timestep = obs.timestep
        H, W = obs.height, obs.width

        target = self._current_target
        self._plan_target = target
        self._plan_turn = timestep

        if target is None:
            self._distance_map = None
            self._cost_map = None
            self._flow_direction = None
            self._gather_map = None
            self._gather_direction = None
            return

        self._distance_map, self._flow_direction = self._bfs_from(target, passable, H, W)
        self._cost_map = self._dijkstra_cost_from(
            target, passable, armies, owned, opponent, fog, structures_fog, timestep, H, W
        )

        gather_point = self._select_gather_point(obs)
        if gather_point is not None:
            self._plan_gather_point = gather_point
            self._gather_map, self._gather_direction = self._bfs_from_owned(
                gather_point, owned, passable, H, W
            )
        else:
            self._gather_map = None
            self._gather_direction = None

    # ------------------------------------------------------------------
    # Strategy target
    # ------------------------------------------------------------------

    def _update_strategy_target(self, obs: Observation) -> None:
        if self._target_commitment > 0:
            self._target_commitment -= 1

        if self._punish_window > 0 and self._punish_target is not None:
            self._current_target = self._punish_target
            self._target_kind = self._punish_kind
            self._target_commitment = max(self._target_commitment, self._punish_window)
            return

        if self._current_target is not None and self._target_kind == "enemy_general" and self._enemy_general_estimate is not None:
            self._current_target = self._enemy_general_estimate

        if self._should_retarget(obs):
            target = self._select_target(obs)
            if target is None:
                self._current_target = None
                self._target_kind = None
                self._target_commitment = 0
            else:
                self._current_target, self._target_kind = target
                self._target_commitment = self._target_commitment_for(self._target_kind)

    def _target_commitment_for(self, kind: str | None) -> int:
        base = {
            "enemy_general": 10,
            "enemy_city": 8,
            "enemy_frontier": 7,
            "frontier_break": 6,
            "structure_fog": 6,
            "neutral_city": 6,
            "unseen": 5,
        }.get(kind or "", 6)
        if self._mode == "ahead":
            base += 2
        elif self._mode == "behind":
            base = max(base - 2, 3)
        return base

    def _should_retarget(self, obs: Observation) -> bool:
        if self._current_target is None or self._target_kind is None:
            return True
        if self._target_commitment <= 0:
            return True
        if self._punish_window > 0 and self._punish_target is not None:
            return tuple(self._current_target) != tuple(self._punish_target)

        row, col = self._current_target
        owned = obs.owned_cells
        opponent = obs.opponent_cells
        cities = obs.cities
        visible = ~(obs.fog_cells | obs.structures_in_fog)

        if self._target_kind == "enemy_general":
            return False
        if visible[row, col]:
            if self._target_kind == "enemy_city":
                return bool(owned[row, col] or not cities[row, col])
            if self._target_kind == "enemy_frontier":
                return not bool(opponent[row, col])
            if self._target_kind == "neutral_city":
                return bool(owned[row, col] or opponent[row, col] or not cities[row, col])
            if self._target_kind in {"structure_fog", "unseen"}:
                return True
        return False

    def _select_target(self, obs: Observation) -> tuple[tuple[int, int], str] | None:
        if self._punish_window > 0 and self._punish_target is not None and self._punish_kind is not None:
            return self._punish_target, self._punish_kind

        if self._enemy_general_estimate is not None:
            return self._enemy_general_estimate, "enemy_general"

        opponent = obs.opponent_cells
        cities = obs.cities
        structures_fog = obs.structures_in_fog
        fog = obs.fog_cells
        owned = obs.owned_cells

        enemy_city_positions = np.argwhere(opponent & cities)
        if len(enemy_city_positions):
            return tuple(map(int, enemy_city_positions[0])), "enemy_city"

        enemy_positions = np.argwhere(opponent)
        if len(enemy_positions):
            best_enemy = max(
                enemy_positions.tolist(),
                key=lambda pos: self._target_priority(obs, tuple(pos), "enemy_frontier"),
            )
            return tuple(map(int, best_enemy)), "enemy_frontier"

        structure_positions = np.argwhere(structures_fog)
        if len(structure_positions):
            best_structure = max(
                structure_positions.tolist(),
                key=lambda pos: self._target_priority(obs, tuple(pos), "structure_fog"),
            )
            return tuple(map(int, best_structure)), "structure_fog"

        neutral_city_positions = np.argwhere(cities & ~owned & ~opponent)
        if len(neutral_city_positions):
            best_city = max(
                neutral_city_positions.tolist(),
                key=lambda pos: self._target_priority(obs, tuple(pos), "neutral_city"),
            )
            return tuple(map(int, best_city)), "neutral_city"

        unseen_positions = np.argwhere(fog)
        if len(unseen_positions):
            best_unseen = max(
                unseen_positions.tolist(),
                key=lambda pos: self._target_priority(obs, tuple(pos), "unseen"),
            )
            return tuple(map(int, best_unseen)), "unseen"
        return None

    def _target_priority(self, obs: Observation, pos: tuple[int, int], kind: str) -> float:
        base_scores = {
            "enemy_general": 300.0,
            # Cities more valuable in new rules (grow every turn)
            "enemy_city": 240.0,
            "enemy_frontier": 180.0,
            "frontier_break": 190.0,
            "structure_fog": 150.0,
            "neutral_city": 140.0,
            "unseen": 90.0,
        }
        score = base_scores[kind]
        if self._own_general is not None:
            home_dist = abs(pos[0] - self._own_general[0]) + abs(pos[1] - self._own_general[1])
            score -= 0.8 * home_dist
        if self._enemy_general_estimate is not None:
            enemy_dist = abs(pos[0] - self._enemy_general_estimate[0]) + abs(pos[1] - self._enemy_general_estimate[1])
            score -= 0.3 * enemy_dist
        if kind in {"enemy_frontier", "enemy_city"}:
            score += 10.0 * self._material_ratio(obs)
        if kind == "neutral_city" and self._material_ratio(obs) < 0.9:
            score -= 20.0
        if self._mode == "ahead" and kind in {"enemy_general", "enemy_city", "enemy_frontier", "frontier_break"}:
            score += 18.0
        if self._mode == "behind" and kind == "neutral_city":
            score += 12.0
        if self._mode == "behind" and kind in {"enemy_frontier", "frontier_break"}:
            score -= 20.0
        if self._punish_window > 0 and self._punish_target is not None and tuple(pos) == tuple(self._punish_target):
            score += 40.0
        return score

    # ------------------------------------------------------------------
    # Punish window
    # ------------------------------------------------------------------

    def _update_punish_window(self, obs: Observation) -> None:
        if self._punish_window > 0:
            self._punish_window -= 1
            if self._punish_window <= 0:
                self._punish_target = None
                self._punish_kind = None
                self._punish_reason = None
                self._punish_strength = 0.0
                self._punish_window = 0

        detection = self._detect_punish_opportunity(obs)
        if detection is None:
            return

        if detection["strength"] >= self._punish_strength or self._punish_window <= 0:
            self._punish_target = detection["target"]
            self._punish_kind = detection["kind"]
            self._punish_reason = detection["reason"]
            self._punish_strength = detection["strength"]
            self._punish_window = detection["window"]

    def _detect_punish_opportunity(self, obs: Observation) -> dict | None:
        owned = obs.owned_cells
        opponent = obs.opponent_cells
        cities = obs.cities
        generals = obs.generals
        armies = obs.armies

        enemy_generals = np.argwhere(generals & opponent)
        if len(enemy_generals):
            target = tuple(map(int, enemy_generals[0]))
            return {
                "target": target,
                "kind": "enemy_general",
                "reason": "visible_enemy_general",
                "strength": 100.0,
                "window": 10,
            }

        enemy_cities = np.argwhere(opponent & cities)
        best_city: tuple[tuple[int, int], float] | None = None
        for row, col in enemy_cities.tolist():
            local_margin = self._local_owned_margin(owned, armies, row, col) - int(armies[row, col])
            if self._mode != "behind" and local_margin >= 2:
                strength = 28.0 + 2.5 * local_margin
                if self._mode == "ahead":
                    strength += 8.0
                if best_city is None or strength > best_city[1]:
                    best_city = ((row, col), strength)
        if best_city is not None:
            return {
                "target": best_city[0],
                "kind": "enemy_city",
                "reason": "visible_enemy_city",
                "strength": best_city[1],
                "window": 6 if self._mode == "ahead" else 4,
            }

        enemy_frontier = np.argwhere(opponent)
        best_frontier: tuple[tuple[int, int], float] | None = None
        for row, col in enemy_frontier.tolist():
            local_margin = self._local_owned_margin(owned, armies, row, col) - int(armies[row, col])
            if local_margin < 4 or self._mode == "behind":
                continue
            dist_term = 0.0
            if self._enemy_general_estimate is not None:
                dist_term = max(0.0, 10.0 - 0.6 * (abs(row - self._enemy_general_estimate[0]) + abs(col - self._enemy_general_estimate[1])))
            strength = 18.0 + 1.8 * local_margin + dist_term
            if best_frontier is None or strength > best_frontier[1]:
                best_frontier = ((row, col), strength)
        if best_frontier is not None:
            return {
                "target": best_frontier[0],
                "kind": "frontier_break",
                "reason": "exposed_enemy_frontier",
                "strength": best_frontier[1],
                "window": 4,
            }
        return None

    def _local_owned_margin(self, owned: np.ndarray, armies: np.ndarray, row: int, col: int) -> int:
        h, w = armies.shape
        margin = 0
        for dr, dc in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
            nr = row + dr
            nc = col + dc
            if 0 <= nr < h and 0 <= nc < w and owned[nr, nc]:
                margin = max(margin, int(armies[nr, nc]) - 1)
        return margin

    # ------------------------------------------------------------------
    # Score adjustments (phase, mode, punish, continuation, anti-loop)
    # ------------------------------------------------------------------

    def _phase_adjustment(self, obs: Observation, move: MoveFeatures) -> float:
        kind = self._classify_move(move)
        score = 0.0
        ratio = self._material_ratio(obs)

        if self._phase == "opening":
            if kind == "expand" and move.can_capture:
                score += 10.0
            if kind == "scout":
                score += 8.0
            if kind in {"reinforce", "reinforce_city"}:
                score -= 6.0
        elif self._phase == "expansion":
            if kind in {"expand", "attack_city"} and move.can_capture:
                score += 8.0
            if kind == "reinforce":
                score -= 4.0
        elif self._phase == "pressure":
            if kind in {"attack", "attack_city", "scout"}:
                score += 8.0
            if kind in {"reinforce", "reinforce_city"} and move.source_army >= 8:
                score -= 6.0
        elif self._phase == "consolidation":
            if ratio > 1.15 and kind in {"attack", "attack_city"} and move.can_capture:
                score += 12.0
            if kind == "scout":
                score += 5.0
            if kind == "reinforce" and move.source_army >= 12:
                score -= 8.0
        elif self._phase == "kill":
            if kind in {"attack", "attack_city"} and move.can_capture:
                score += 16.0
            if self._enemy_general_estimate is not None:
                source_enemy_dist = abs(move.row - self._enemy_general_estimate[0]) + abs(move.col - self._enemy_general_estimate[1])
                dest_enemy_dist = self._distance_to_enemy_estimate(move)
                score += 5.0 * (source_enemy_dist - dest_enemy_dist)
            if kind in {"reinforce", "reinforce_city"}:
                score -= 12.0
        elif self._phase == "defense":
            if self._own_general is not None:
                source_home_dist = abs(move.row - self._own_general[0]) + abs(move.col - self._own_general[1])
                dest_home_dist = self._distance_from_own_general(move)
                if dest_home_dist < source_home_dist and kind in {"reinforce", "attack"}:
                    score += 10.0
            if kind == "attack_city" and ratio < 0.9:
                score -= 12.0

        return score

    def _mode_adjustment(self, obs: Observation, move: MoveFeatures) -> float:
        kind = self._classify_move(move)
        score = 0.0

        if self._mode == "ahead":
            if kind in {"attack", "attack_city", "attack_general"} and move.can_capture:
                score += 10.0
            if kind == "scout" and move.source_army >= 6:
                score += 4.0
            if kind in {"reinforce", "reinforce_city"} and move.source_army >= 10:
                score -= 8.0
        elif self._mode == "behind":
            if self._own_general is not None:
                source_home_dist = abs(move.row - self._own_general[0]) + abs(move.col - self._own_general[1])
                dest_home_dist = self._distance_from_own_general(move)
                if dest_home_dist < source_home_dist and kind in {"reinforce", "attack"}:
                    score += 8.0
            if kind in {"attack_city", "scout"} and move.source_army >= 10:
                score -= 10.0
            if kind == "reinforce" and move.source_army >= 8:
                score += 4.0
        else:
            if kind in {"attack", "attack_city"} and move.can_capture:
                score += 3.0
            if kind == "reinforce" and move.source_army >= 14:
                score -= 3.0

        return score

    def _punish_adjustment(self, obs: Observation, move: MoveFeatures) -> float:
        if self._punish_window <= 0 or self._punish_target is None:
            return 0.0

        kind = self._classify_move(move)
        source_dist = abs(move.row - self._punish_target[0]) + abs(move.col - self._punish_target[1])
        dest_dist = abs(move.dest_row - self._punish_target[0]) + abs(move.dest_col - self._punish_target[1])
        progress = source_dist - dest_dist
        score = 6.0 * progress
        if progress > 0 and kind in {"attack", "attack_city", "attack_general", "scout", "expand"}:
            score += 8.0
        if progress < 0 and kind in {"reinforce", "reinforce_city"} and move.source_army >= 8:
            score += 6.0 * progress

        if self._punish_kind == "enemy_city":
            if move.dest_is_city and move.can_capture:
                score += 22.0
            elif kind in {"reinforce", "reinforce_city"}:
                score -= 6.0
        elif self._punish_kind == "frontier_break":
            if move.dest_is_opponent and move.can_capture:
                score += 16.0
            if kind == "scout" and move.source_army < 6:
                score -= 4.0
        elif self._punish_kind == "enemy_general":
            if kind == "attack_general" and move.can_capture:
                score += 1000.0
            elif kind in {"reinforce", "reinforce_city"}:
                score -= 10.0

        return score

    def _continuation_value(self, obs: Observation, move: MoveFeatures) -> float:
        armies = obs.armies
        owned = obs.owned_cells
        opponent = obs.opponent_cells
        neutral = obs.neutral_cells
        cities = obs.cities
        fog = obs.fog_cells
        structures_fog = obs.structures_in_fog
        mountains = obs.mountains
        h, w = obs.height, obs.width
        score = 0.0

        for dr, dc in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
            nr = move.dest_row + dr
            nc = move.dest_col + dc
            if nr < 0 or nr >= h or nc < 0 or nc >= w or mountains[nr, nc]:
                continue
            if opponent[nr, nc]:
                score += 4.5
                if move.moving_army > int(armies[nr, nc]):
                    score += 2.0
            elif neutral[nr, nc]:
                score += 2.5
                if move.moving_army > int(armies[nr, nc]):
                    score += 1.0
            elif cities[nr, nc] and not owned[nr, nc]:
                score += 5.0
            elif fog[nr, nc] or structures_fog[nr, nc]:
                score += 2.0
            elif owned[nr, nc]:
                score += 0.5

        if self._current_target is not None:
            score += max(0.0, 6.0 - self._distance_to_target(move.dest_row, move.dest_col) * 0.5)
        if move.dest_is_opponent and move.can_capture:
            score += 3.0
        if move.dest_is_city and move.can_capture:
            score += 3.0
        return score

    def _score_adjustment(self, obs: Observation, move: MoveFeatures) -> float:
        score = 0.0
        dest = (move.dest_row, move.dest_col)
        source = (move.row, move.col)
        kind = self._classify_move(move)
        recent_choices = list(self._recent_choices)
        recent_kinds = list(self._recent_action_kinds)

        if recent_choices:
            last = recent_choices[-1]
            if source == last["dest"] and dest == last["source"]:
                score -= 28.0
            if dest == last["source"]:
                score -= 10.0
            if dest == last["dest"] and kind in {"reinforce", "reinforce_city"}:
                score -= 8.0

        if len(recent_choices) >= 4:
            recent_sources = [choice["source"] for choice in recent_choices[-4:]]
            recent_dests = [choice["dest"] for choice in recent_choices[-4:]]
            if dest in recent_sources[-3:] or dest in recent_dests[-3:]:
                score -= 6.0

        stagnating = len(recent_kinds) >= 4 and all(k in {"reinforce", "reinforce_city"} for k in recent_kinds[-4:])
        if stagnating:
            if kind in {"attack", "attack_city", "scout", "expand"} and move.source_army >= 6:
                score += 16.0
            if kind in {"reinforce", "reinforce_city"} and move.source_army >= 10:
                score -= 12.0

        if move.dest_is_owned and move.source_army >= 18:
            score -= 6.0
        if move.dest_is_opponent and move.can_capture:
            score += 8.0
        if move.dest_is_city and move.can_capture:
            score += 6.0
        if move.dest_is_fog and move.source_army >= 6:
            score += 4.0

        if self._enemy_general_estimate is not None:
            source_enemy_dist = abs(move.row - self._enemy_general_estimate[0]) + abs(move.col - self._enemy_general_estimate[1])
            dest_enemy_dist = self._distance_to_enemy_estimate(move)
            if dest_enemy_dist < source_enemy_dist and not move.dest_is_owned:
                score += 3.0

        if self._current_target is not None:
            source_target_dist = self._distance_to_target(move.row, move.col)
            dest_target_dist = self._distance_to_target(move.dest_row, move.dest_col)
            progress = source_target_dist - dest_target_dist
            score += 4.5 * progress
            if progress < 0 and kind in {"reinforce", "reinforce_city"} and move.source_army >= 8:
                score += 5.0 * progress
            if progress > 0 and kind in {"attack", "attack_city", "scout", "expand"}:
                score += 6.0
            if self._target_kind == "enemy_city" and move.dest_is_city and move.can_capture:
                score += 18.0
            if self._target_kind == "enemy_frontier" and move.dest_is_opponent and move.can_capture:
                score += 14.0
            if self._target_kind in {"structure_fog", "unseen"} and kind == "scout":
                score += 10.0

        if self._own_general is not None and move.dest_is_owned:
            source_home_dist = abs(move.row - self._own_general[0]) + abs(move.col - self._own_general[1])
            dest_home_dist = self._distance_from_own_general(move)
            if source_home_dist <= 2 and dest_home_dist <= 2 and move.source_army >= 12:
                score -= 8.0

        if kind == "expand" and move.source_army < 3:
            score -= 6.0

        return score

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    def _classify_move(self, move: MoveFeatures) -> str:
        if move.dest_is_general and move.can_capture:
            return "attack_general"
        if move.dest_is_city:
            return "attack_city" if (move.dest_is_opponent or move.dest_is_neutral) else "reinforce_city"
        if move.dest_is_opponent:
            return "attack"
        if move.dest_is_neutral:
            return "expand"
        if move.dest_is_fog or move.dest_is_structure_fog:
            return "scout"
        if move.dest_is_owned:
            return "reinforce"
        return "move"

    def _remember_choice(self, move: MoveFeatures, kind: str) -> None:
        self._recent_choices.append({
            "source": (move.row, move.col),
            "dest": (move.dest_row, move.dest_col),
            "kind": kind,
        })
        self._recent_action_kinds.append(kind)

    def _distance_to_enemy_estimate(self, move: MoveFeatures) -> int:
        if self._enemy_general_estimate is None:
            return 0
        return abs(move.dest_row - self._enemy_general_estimate[0]) + abs(move.dest_col - self._enemy_general_estimate[1])

    def _distance_from_own_general(self, move: MoveFeatures) -> int:
        if self._own_general is None:
            return 0
        return abs(move.dest_row - self._own_general[0]) + abs(move.dest_col - self._own_general[1])

    def _distance_to_target(self, row: int, col: int) -> int:
        if self._current_target is None:
            return 0
        return abs(row - self._current_target[0]) + abs(col - self._current_target[1])
