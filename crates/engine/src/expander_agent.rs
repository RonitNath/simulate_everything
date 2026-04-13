use std::collections::VecDeque;

use rand::RngCore;

use crate::action::{Action, Direction};
use crate::agent::{Agent, Observation, valid_dirs};
use crate::state::Tile;

/// Economy-first agent with phase transitions.
///
/// Three phases:
/// - **Expand**: Aggressively claim territory and cities. Cities are 27x the
///   income of empty land per wave cycle — capturing them early compounds.
/// - **Pressure**: Enemy contacted. Continue expanding but orient toward enemy.
///   Defend borders while building economic advantage.
/// - **Strike**: Economic advantage established. Concentrate all force on a
///   single attack axis toward enemy general. Convert economy into a kill.
///
/// Philosophy: land and cities compound via wave turns. Build overwhelming
/// economy, then convert it into a decisive attack. Patient early, lethal late.
pub struct ExpanderAgent {
    memory_owner: Vec<Option<u8>>,
    memory_army: Vec<i32>,
    memory_turn: Vec<u32>,
    own_general: Option<usize>,
    enemy_generals: Vec<Option<usize>>,
    initialized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Expand,
    Pressure,
    Strike,
}

impl ExpanderAgent {
    pub fn new() -> Self {
        Self {
            memory_owner: Vec::new(),
            memory_army: Vec::new(),
            memory_turn: Vec::new(),
            own_general: None,
            enemy_generals: vec![None; 8],
            initialized: false,
        }
    }

    fn init_memory(&mut self, obs: &Observation) {
        let n = obs.width * obs.height;
        self.memory_owner = vec![None; n];
        self.memory_army = vec![0; n];
        self.memory_turn = vec![0; n];
        self.enemy_generals = vec![None; 8];
        self.initialized = true;
    }

    fn update_memory(&mut self, obs: &Observation) {
        if !self.initialized {
            self.init_memory(obs);
        }
        let n = obs.width * obs.height;
        for i in 0..n {
            if obs.visible[i] {
                self.memory_owner[i] = obs.owners[i];
                self.memory_army[i] = obs.armies[i];
                self.memory_turn[i] = obs.turn;

                if obs.tiles[i] == Tile::General {
                    if obs.owners[i] == Some(obs.player) {
                        self.own_general = Some(i);
                    } else if let Some(owner) = obs.owners[i] {
                        self.enemy_generals[owner as usize] = Some(i);
                    }
                }
            }
        }
    }

    fn has_enemy_visible(&self, obs: &Observation) -> bool {
        let n = obs.width * obs.height;
        (0..n).any(|i| obs.visible[i] && obs.owners[i].is_some_and(|o| o != obs.player))
    }

    fn detect_phase(&self, obs: &Observation, has_enemy: bool) -> Phase {
        if !has_enemy && obs.turn < 75 {
            return Phase::Expand;
        }

        let best_opponent_land = obs
            .opponent_stats
            .iter()
            .map(|&(_, land, _)| land)
            .max()
            .unwrap_or(0);
        let best_opponent_army = obs
            .opponent_stats
            .iter()
            .map(|&(_, _, army)| army)
            .max()
            .unwrap_or(0);

        let land_ratio = if best_opponent_land > 0 {
            obs.my_land as f32 / best_opponent_land as f32
        } else {
            2.0
        };
        let army_ratio = if best_opponent_army > 0 {
            obs.my_armies as f32 / best_opponent_army as f32
        } else {
            2.0
        };

        // Strike: even a modest advantage should trigger offense. Don't wait
        // for 1.3x — convert any lead into pressure before it stagnates.
        // Also strike after T150 regardless, since waiting longer lets pressure
        // agents find and assassinate our general.
        let has_target = self.enemy_generals.iter().any(|g| g.is_some()) || has_enemy;
        if has_target && (land_ratio >= 1.15 || army_ratio >= 1.2 || obs.turn > 150) {
            return Phase::Strike;
        }

        if has_enemy {
            Phase::Pressure
        } else {
            Phase::Expand
        }
    }

    fn best_attack_target(&self, obs: &Observation) -> Option<usize> {
        // Known enemy general — highest priority.
        for entry in &self.enemy_generals {
            if let Some(idx) = entry {
                if self.memory_owner[*idx].is_some_and(|o| o != obs.player) {
                    return Some(*idx);
                }
            }
        }
        // Estimate: centroid of visible enemy cells.
        let w = obs.width;
        let n = w * obs.height;
        let mut sum_r: usize = 0;
        let mut sum_c: usize = 0;
        let mut count: usize = 0;
        for i in 0..n {
            if obs.visible[i] && obs.owners[i].is_some_and(|o| o != obs.player) {
                sum_r += i / w;
                sum_c += i % w;
                count += 1;
            }
        }
        if count > 0 {
            Some((sum_r / count) * w + (sum_c / count))
        } else {
            None
        }
    }

    fn bfs_multi(obs: &Observation, seeds: &[usize]) -> Vec<u32> {
        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();
        let w = obs.width;
        let h = obs.height;
        for &idx in seeds {
            if idx < n {
                dist[idx] = 0;
                queue.push_back(idx);
            }
        }
        while let Some(idx) = queue.pop_front() {
            let d = dist[idx] + 1;
            let row = idx / w;
            let col = idx % w;
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = row as i32 + dr;
                let nc = col as i32 + dc;
                if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                    let ni = nr as usize * w + nc as usize;
                    if obs.tiles[ni] != Tile::Mountain && dist[ni] > d {
                        dist[ni] = d;
                        queue.push_back(ni);
                    }
                }
            }
        }
        dist
    }

    fn bfs_owned(obs: &Observation, seeds: &[usize]) -> Vec<u32> {
        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();
        let w = obs.width;
        let h = obs.height;
        for &idx in seeds {
            if idx < n && obs.owners[idx] == Some(obs.player) {
                dist[idx] = 0;
                queue.push_back(idx);
            }
        }
        while let Some(idx) = queue.pop_front() {
            let d = dist[idx] + 1;
            let row = idx / w;
            let col = idx % w;
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = row as i32 + dr;
                let nc = col as i32 + dc;
                if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                    let ni = nr as usize * w + nc as usize;
                    if obs.owners[ni] == Some(obs.player) && dist[ni] > d {
                        dist[ni] = d;
                        queue.push_back(ni);
                    }
                }
            }
        }
        dist
    }

    /// Scan for visible cities we could capture — even if not adjacent.
    /// Returns BFS distance from each cell to the nearest reachable uncaptured city.
    fn city_attraction(obs: &Observation) -> Vec<u32> {
        let n = obs.width * obs.height;
        let w = obs.width;
        let h = obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();

        for i in 0..n {
            if obs.tiles[i] == Tile::City && obs.owners[i] != Some(obs.player) {
                dist[i] = 0;
                queue.push_back(i);
            }
        }

        while let Some(idx) = queue.pop_front() {
            let d = dist[idx] + 1;
            let row = idx / w;
            let col = idx % w;
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = row as i32 + dr;
                let nc = col as i32 + dc;
                if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                    let ni = nr as usize * w + nc as usize;
                    if obs.tiles[ni] != Tile::Mountain && dist[ni] > d {
                        dist[ni] = d;
                        queue.push_back(ni);
                    }
                }
            }
        }
        dist
    }

    /// Detect if our general is under threat: enemy with significant army
    /// within 8 BFS steps through any passable terrain.
    fn general_under_threat(&self, obs: &Observation) -> bool {
        let general_idx = match self.own_general {
            Some(idx) => idx,
            None => return false,
        };
        let w = obs.width;
        let h = obs.height;
        let dist = Self::bfs_multi(obs, &[general_idx]);
        let gen_army = obs.armies[general_idx];
        for i in 0..w * h {
            if dist[i] <= 8 && obs.visible[i] && obs.owners[i].is_some_and(|o| o != obs.player) {
                // Threat if enemy army could beat our general garrison.
                if obs.armies[i] > gen_army / 2 && obs.armies[i] >= 5 {
                    return true;
                }
            }
        }
        false
    }

    fn emit_orders(&self, obs: &Observation, phase: Phase) -> Vec<Action> {
        let w = obs.width;
        let h = obs.height;
        let center_r = h as f32 / 2.0;
        let center_c = w as f32 / 2.0;

        let defending = self.general_under_threat(obs);

        // Build frontier.
        let mut all_frontier = Vec::new();
        for row in 0..h {
            for col in 0..w {
                let idx = row * w + col;
                if obs.owners[idx] != Some(obs.player) {
                    continue;
                }
                let is_frontier = Direction::ALL.iter().any(|dir| {
                    let (dr, dc) = dir.delta();
                    let nr = row as i32 + dr;
                    let nc = col as i32 + dc;
                    nr >= 0
                        && nc >= 0
                        && (nr as usize) < h
                        && (nc as usize) < w
                        && {
                            let ni = nr as usize * w + nc as usize;
                            obs.tiles[ni] != Tile::Mountain && obs.owners[ni] != Some(obs.player)
                        }
                });
                if is_frontier {
                    all_frontier.push(idx);
                }
            }
        }

        let frontier_dist = Self::bfs_owned(obs, &all_frontier);
        let city_dist = Self::city_attraction(obs);

        // Consolidation target depends on situation:
        // - Defending: rally to general
        // - Strike: concentrate on attack axis (top 25% of frontier nearest target)
        // - Otherwise: toward frontier (for expansion)
        let consolidation_dist = if defending {
            if let Some(gen_idx) = self.own_general {
                Self::bfs_owned(obs, &[gen_idx])
            } else {
                frontier_dist.clone()
            }
        } else if phase == Phase::Strike {
            if let Some(target) = self.best_attack_target(obs) {
                let target_dist = Self::bfs_multi(obs, &[target]);
                let mut frontier_target_dists: Vec<u32> = all_frontier
                    .iter()
                    .map(|&idx| target_dist[idx])
                    .filter(|&d| d < u32::MAX)
                    .collect();
                if frontier_target_dists.is_empty() {
                    frontier_dist.clone()
                } else {
                    frontier_target_dists.sort();
                    // Top 25% of frontier cells nearest the target.
                    let cutoff_idx = (frontier_target_dists.len() / 4)
                        .max(1)
                        .min(frontier_target_dists.len());
                    let cutoff = frontier_target_dists[cutoff_idx - 1];
                    let attack_frontier: Vec<usize> = all_frontier
                        .iter()
                        .filter(|&&idx| target_dist[idx] <= cutoff)
                        .copied()
                        .collect();
                    Self::bfs_owned(obs, &attack_frontier)
                }
            } else {
                frontier_dist.clone()
            }
        } else {
            frontier_dist.clone()
        };

        let mut orders = Vec::new();

        for row in 0..h {
            for col in 0..w {
                let idx = row * w + col;
                if obs.owners[idx] != Some(obs.player) || obs.armies[idx] <= 1 {
                    continue;
                }

                let my_army = obs.armies[idx];
                let my_fd = frontier_dist[idx];
                let is_frontier = my_fd == 0;
                let dirs = valid_dirs(obs, row, col);
                if dirs.is_empty() {
                    continue;
                }

                if is_frontier {
                    // === FRONTIER CELL: score outward targets ===
                    let mut best_score = i32::MIN;
                    let mut best_dir = None;

                    for &dir in &dirs {
                        let (dr, dc) = dir.delta();
                        let nr = (row as i32 + dr) as usize;
                        let nc = (col as i32 + dc) as usize;
                        let ni = nr * w + nc;

                        if obs.owners[ni] == Some(obs.player) {
                            continue;
                        }

                        let target_army = obs.armies[ni];
                        let target_tile = obs.tiles[ni];
                        let is_enemy = obs.owners[ni].is_some_and(|o| o != obs.player);
                        let can_capture = my_army - 1 > target_army;

                        let score = if target_tile == Tile::General && is_enemy && can_capture {
                            100_000
                        } else if target_tile == Tile::City && can_capture {
                            // Cities are the key to expander's strategy.
                            // Massive priority in Expand, still high later.
                            match phase {
                                Phase::Expand => 5000,
                                Phase::Pressure => 4000,
                                Phase::Strike => 3000,
                            }
                        } else if is_enemy && can_capture {
                            match phase {
                                Phase::Expand => 500,
                                Phase::Pressure => 1500,
                                Phase::Strike => 2500,
                            }
                        } else if !is_enemy && can_capture {
                            // Empty land.
                            let mut s = match phase {
                                Phase::Expand => 1000,
                                Phase::Pressure => 600,
                                Phase::Strike => 200,
                            };
                            // Bonus for expanding toward visible cities.
                            if phase != Phase::Strike && city_dist[ni] < 6 {
                                s += (600 - city_dist[ni] as i32 * 100).max(0);
                            }
                            s
                        } else {
                            -100
                        };

                        // Center bias during expand phase.
                        let score = if phase == Phase::Expand {
                            let dist_to_center = ((nr as f32 - center_r).abs()
                                + (nc as f32 - center_c).abs())
                                as i32;
                            score - dist_to_center * 5
                        } else {
                            score
                        };

                        if score > best_score {
                            best_score = score;
                            best_dir = Some(dir);
                        }
                    }

                    if let Some(dir) = best_dir {
                        if best_score >= 0 {
                            orders.push(Action::Move {
                                row,
                                col,
                                dir,
                                split: false,
                            });
                            continue;
                        }
                    }

                    // Frontier with no good outward move: consolidate inward
                    // toward the attack axis (Strike) or just sit tight.
                    if my_army >= 2 && (phase == Phase::Strike || phase == Phase::Pressure) {
                        let my_cd = consolidation_dist[idx];
                        if let Some(dir) = dirs
                            .iter()
                            .copied()
                            .filter(|&dir| {
                                let (dr, dc) = dir.delta();
                                let nr = (row as i32 + dr) as usize;
                                let nc = (col as i32 + dc) as usize;
                                let ni = nr * w + nc;
                                obs.owners[ni] == Some(obs.player)
                                    && consolidation_dist[ni] < my_cd
                            })
                            .min_by_key(|&dir| {
                                let (dr, dc) = dir.delta();
                                let nr = (row as i32 + dr) as usize;
                                let nc = (col as i32 + dc) as usize;
                                consolidation_dist[nr * w + nc]
                            })
                        {
                            orders.push(Action::Move {
                                row,
                                col,
                                dir,
                                split: false,
                            });
                        }
                    }
                } else {
                    // === INTERIOR CELL: consolidate ===
                    let min_army = match phase {
                        Phase::Expand => 3,
                        Phase::Pressure | Phase::Strike => 2,
                    };
                    if my_army < min_army {
                        continue;
                    }

                    let my_cd = consolidation_dist[idx];
                    if let Some(dir) = dirs
                        .iter()
                        .copied()
                        .filter(|&dir| {
                            let (dr, dc) = dir.delta();
                            let nr = (row as i32 + dr) as usize;
                            let nc = (col as i32 + dc) as usize;
                            let ni = nr * w + nc;
                            obs.owners[ni] == Some(obs.player) && consolidation_dist[ni] < my_cd
                        })
                        .min_by_key(|&dir| {
                            let (dr, dc) = dir.delta();
                            let nr = (row as i32 + dr) as usize;
                            let nc = (col as i32 + dc) as usize;
                            consolidation_dist[nr * w + nc]
                        })
                    {
                        orders.push(Action::Move {
                            row,
                            col,
                            dir,
                            split: false,
                        });
                    }
                }
            }
        }

        orders
    }
}

impl Agent for ExpanderAgent {
    fn name(&self) -> &str {
        "expander"
    }

    fn version(&self) -> &str {
        "v2"
    }

    fn act(&mut self, obs: &Observation, _rng: &mut dyn RngCore) -> Vec<Action> {
        self.update_memory(obs);
        let phase = self.detect_phase(obs, self.has_enemy_visible(obs));
        self.emit_orders(obs, phase)
    }

    fn reset(&mut self) {
        self.memory_owner.clear();
        self.memory_army.clear();
        self.memory_turn.clear();
        self.own_general = None;
        self.enemy_generals.clear();
        self.initialized = false;
    }
}
