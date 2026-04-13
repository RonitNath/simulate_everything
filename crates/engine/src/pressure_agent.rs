use std::collections::VecDeque;

use rand::RngCore;

use crate::action::{Action, Direction};
use crate::agent::{Agent, Observation};
use crate::state::Tile;

/// Role-based agent with world model, fog memory, and targeted consolidation.
///
/// Two phases:
/// - **Frontier cells** score adjacent non-owned cells and expand/attack.
/// - **Interior cells** consolidate toward a target: enemy general if known,
///   attack frontier if enemy visible, nearest frontier otherwise.
///
/// Special modes: general defense when threatened, city-hungry expansion.
pub struct PressureAgent {
    /// Per-player model, rebuilt each turn from visible information.
    player_models: Vec<PlayerModel>,
    /// Fog-of-war memory: last known owner per cell (persists across turns).
    memory_owner: Vec<Option<u8>>,
    /// Fog-of-war memory: last known army per cell.
    memory_army: Vec<i32>,
    /// Turn when each cell was last visible.
    memory_turn: Vec<u32>,
    /// Our general position, once discovered.
    own_general: Option<usize>,
    /// Known/estimated enemy general positions: player_id -> cell index.
    enemy_generals: Vec<Option<usize>>,
    /// Whether memory has been initialized.
    initialized: bool,
}

#[derive(Debug, Clone)]
struct PlayerModel {
    id: u8,
    total_army: i32,
    total_land: usize,
    visible_army: i32,
    visible_land: usize,
    /// Our cells that border this player's visible cells.
    border_cells: Vec<usize>,
    /// Their army strength along our shared border.
    border_threat: i32,
    /// Our army strength along our shared border.
    border_defense: i32,
    /// their total_army / our total_army
    threat_ratio: f32,
}

impl PressureAgent {
    pub fn new() -> Self {
        Self {
            player_models: Vec::new(),
            memory_owner: Vec::new(),
            memory_army: Vec::new(),
            memory_turn: Vec::new(),
            own_general: None,
            enemy_generals: Vec::new(),
            initialized: false,
        }
    }

    fn init_memory(&mut self, obs: &Observation) {
        let n = obs.width * obs.height;
        self.memory_owner = vec![None; n];
        self.memory_army = vec![0; n];
        self.memory_turn = vec![0; n];
        self.enemy_generals = vec![None; 16];
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

    fn build_player_models(&mut self, obs: &Observation) {
        self.player_models.clear();
        let w = obs.width;
        let h = obs.height;

        for &(pid, land, army) in &obs.opponent_stats {
            let mut model = PlayerModel {
                id: pid,
                total_army: army,
                total_land: land,
                visible_army: 0,
                visible_land: 0,
                border_cells: Vec::new(),
                border_threat: 0,
                border_defense: 0,
                threat_ratio: if obs.my_armies > 0 {
                    army as f32 / obs.my_armies as f32
                } else {
                    10.0
                },
            };

            for i in 0..w * h {
                if obs.owners[i] == Some(pid) && obs.visible[i] {
                    model.visible_army += obs.armies[i];
                    model.visible_land += 1;
                }
            }

            for row in 0..h {
                for col in 0..w {
                    let idx = row * w + col;
                    if obs.owners[idx] != Some(obs.player) {
                        continue;
                    }
                    let mut touches_enemy = false;
                    for dir in Direction::ALL {
                        let (dr, dc) = dir.delta();
                        let nr = row as i32 + dr;
                        let nc = col as i32 + dc;
                        if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                            let ni = nr as usize * w + nc as usize;
                            if obs.owners[ni] == Some(pid) && obs.visible[ni] {
                                touches_enemy = true;
                                model.border_threat += obs.armies[ni];
                            }
                        }
                    }
                    if touches_enemy {
                        model.border_cells.push(idx);
                        model.border_defense += obs.armies[idx];
                    }
                }
            }

            self.player_models.push(model);
        }
    }

    /// BFS through any passable terrain (not mountains).
    fn bfs_multi(obs: &Observation, seeds: &[usize]) -> Vec<u32> {
        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();
        let w = obs.width;
        let h = obs.height;

        for &idx in seeds {
            dist[idx] = 0;
            queue.push_back(idx);
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

    /// BFS only through owned territory.
    fn bfs_owned(obs: &Observation, seeds: &[usize]) -> Vec<u32> {
        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();
        let w = obs.width;
        let h = obs.height;

        for &idx in seeds {
            if obs.owners[idx] == Some(obs.player) {
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

    /// Find the best known enemy general position, or estimate from visible enemy cells.
    fn best_attack_target(&self, obs: &Observation) -> Option<usize> {
        // Known enemy general — highest priority.
        for entry in &self.enemy_generals {
            if let Some(idx) = entry {
                // Verify it's still an enemy general (not captured).
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
            let cr = sum_r / count;
            let cc = sum_c / count;
            Some(cr * w + cc)
        } else {
            None
        }
    }

    /// Detect if our general is under threat: enemy army within N BFS steps
    /// through any terrain, with enough strength to be dangerous.
    fn general_threat_level(&self, obs: &Observation) -> Option<(usize, i32)> {
        let general_idx = self.own_general?;
        let w = obs.width;
        let h = obs.height;

        // BFS from general through all passable terrain, check within 5 tiles.
        let dist = Self::bfs_multi(obs, &[general_idx]);
        let mut max_threat = 0i32;
        for i in 0..w * h {
            if dist[i] <= 5 && obs.visible[i] {
                if obs.owners[i].is_some_and(|o| o != obs.player) {
                    max_threat = max_threat.max(obs.armies[i]);
                }
            }
        }
        if max_threat >= 5 {
            Some((general_idx, max_threat))
        } else {
            None
        }
    }

    /// Detect marauders: heavy enemy stacks inside or near our territory.
    /// Returns their cell indices.
    fn find_marauders(&self, obs: &Observation) -> Vec<usize> {
        let w = obs.width;
        let h = obs.height;
        let mut marauders = Vec::new();

        for i in 0..w * h {
            if !obs.visible[i] || obs.armies[i] < 8 {
                continue;
            }
            if !obs.owners[i].is_some_and(|o| o != obs.player) {
                continue;
            }
            // Count how many passable neighbors are ours.
            let row = i / w;
            let col = i % w;
            let mut own_neighbors = 0u8;
            let mut total_neighbors = 0u8;
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = row as i32 + dr;
                let nc = col as i32 + dc;
                if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                    let ni = nr as usize * w + nc as usize;
                    if obs.tiles[ni] == Tile::Mountain {
                        continue;
                    }
                    total_neighbors += 1;
                    if obs.owners[ni] == Some(obs.player) {
                        own_neighbors += 1;
                    }
                }
            }
            // Marauder: enemy cell where most neighbors are ours (deep in our territory).
            if total_neighbors > 0 && own_neighbors >= 2 {
                marauders.push(i);
            }
        }
        marauders
    }

    /// Role-based order emission with single-objective focus and marauder interception.
    fn emit_orders(&self, obs: &Observation, _rng: &mut dyn RngCore) -> Vec<Action> {
        let w = obs.width;
        let h = obs.height;
        let n = w * h;
        let center_r = h as f32 / 2.0;
        let center_c = w as f32 / 2.0;

        let defense_mode = self.general_threat_level(obs);
        let marauders = self.find_marauders(obs);

        // Classify frontier cells.
        let mut all_frontier = Vec::new();
        let mut has_enemy_visible = false;

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
                    nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w && {
                        let ni = nr as usize * w + nc as usize;
                        obs.tiles[ni] != Tile::Mountain && obs.owners[ni] != Some(obs.player)
                    }
                });
                if is_frontier {
                    all_frontier.push(idx);
                }
                if !has_enemy_visible {
                    for dir in Direction::ALL {
                        let (dr, dc) = dir.delta();
                        let nr = row as i32 + dr;
                        let nc = col as i32 + dc;
                        if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                            let ni = nr as usize * w + nc as usize;
                            if obs.visible[ni] && obs.owners[ni].is_some_and(|o| o != obs.player) {
                                has_enemy_visible = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        let frontier_dist = Self::bfs_owned(obs, &all_frontier);

        // === SINGLE ATTACK OBJECTIVE ===
        // Pick the best target, then select only the nearest ~25% of frontier
        // cells to that target as the consolidation sink. This focuses all army
        // onto one narrow attack axis instead of spreading across the entire border.
        let attack_target = self.best_attack_target(obs);

        let objective_dist = if let Some((general_idx, _)) = defense_mode {
            Self::bfs_owned(obs, &[general_idx])
        } else if let Some(target) = attack_target {
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
                // Tight focus: top 25% of frontier cells nearest the target.
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
        };

        // === MARAUDER INTERCEPTION ===
        // BFS from marauder positions through owned territory.
        // Cells within intercept_radius route to marauder instead of main objective.
        let intercept_radius: u32 = 6;
        let marauder_dist = if marauders.is_empty() {
            vec![u32::MAX; n]
        } else {
            // BFS from marauder-adjacent owned cells through owned territory.
            let mut intercept_seeds = Vec::new();
            for &mi in &marauders {
                let mrow = mi / w;
                let mcol = mi % w;
                for dir in Direction::ALL {
                    let (dr, dc) = dir.delta();
                    let nr = mrow as i32 + dr;
                    let nc = mcol as i32 + dc;
                    if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                        let ni = nr as usize * w + nc as usize;
                        if obs.owners[ni] == Some(obs.player) {
                            intercept_seeds.push(ni);
                        }
                    }
                }
            }
            Self::bfs_owned(obs, &intercept_seeds)
        };

        // Per-cell consolidation target: intercept if close to marauder, else main objective.
        let consolidation_dist: Vec<u32> = (0..n)
            .map(|i| {
                if marauder_dist[i] < intercept_radius && marauder_dist[i] < objective_dist[i] {
                    marauder_dist[i]
                } else {
                    objective_dist[i]
                }
            })
            .collect();

        struct Order {
            row: usize,
            col: usize,
            dir: Direction,
            priority: f32,
        }

        let mut orders: Vec<Order> = Vec::new();

        for row in 0..h {
            for col in 0..w {
                let idx = row * w + col;
                if obs.owners[idx] != Some(obs.player) || obs.armies[idx] <= 1 {
                    continue;
                }

                let my_army = obs.armies[idx];
                let my_fd = frontier_dist[idx];
                let is_frontier = my_fd == 0;

                if is_frontier {
                    // === FRONTIER CELL ===
                    // Try to expand/attack outward.
                    let mut best_dir = None;
                    let mut best_score = i32::MIN;
                    let mut best_dest_army: i32 = 0;
                    let mut best_dest_is_defended = false;

                    for dir in Direction::ALL {
                        let (dr, dc) = dir.delta();
                        let nr = row as i32 + dr;
                        let nc = col as i32 + dc;
                        if nr < 0 || nc < 0 || (nr as usize) >= h || (nc as usize) >= w {
                            continue;
                        }
                        let ni = nr as usize * w + nc as usize;
                        if obs.tiles[ni] == Tile::Mountain || obs.owners[ni] == Some(obs.player) {
                            continue;
                        }

                        let dest_army = obs.armies[ni];
                        let can_capture = my_army - 1 > dest_army;
                        let dest_tile = obs.tiles[ni];
                        let is_enemy = obs.owners[ni].is_some_and(|o| o != obs.player);
                        let is_defended = is_enemy || dest_tile == Tile::City;

                        let mut score = if dest_tile == Tile::General && is_enemy && can_capture {
                            100_000
                        } else if dest_tile == Tile::City && can_capture {
                            5000
                        } else if is_enemy && can_capture {
                            2000
                        } else if !is_defended && can_capture {
                            800
                        } else if !obs.visible[ni] && can_capture {
                            700
                        } else if is_defended && can_capture {
                            600
                        } else {
                            -100
                        };

                        // Center bias before enemy contact.
                        if !has_enemy_visible {
                            let dist_to_center = ((nr as f32 - center_r).abs()
                                + (nc as f32 - center_c).abs())
                                as i32;
                            score -= dist_to_center * 5;
                        }

                        if score > best_score {
                            best_score = score;
                            best_dir = Some(dir);
                            best_dest_army = dest_army;
                            best_dest_is_defended = is_defended;
                        }
                    }

                    if let Some(dir) = best_dir {
                        let can_act = if best_dest_is_defended {
                            my_army - 1 > best_dest_army
                        } else {
                            best_score >= 0
                        };

                        if can_act {
                            orders.push(Order {
                                row,
                                col,
                                dir,
                                priority: best_score as f32 + my_army as f32 * 0.1,
                            });
                            continue;
                        }
                    }

                    // Frontier cell with no good outward move: consolidate inward
                    // toward the objective instead of sitting idle.
                    if my_army >= 2 {
                        let my_cd = consolidation_dist[idx];
                        let mut best_cd_dir = None;
                        let mut best_cd = my_cd;
                        for dir in Direction::ALL {
                            let (dr, dc) = dir.delta();
                            let nr = row as i32 + dr;
                            let nc = col as i32 + dc;
                            if nr < 0 || nc < 0 || (nr as usize) >= h || (nc as usize) >= w {
                                continue;
                            }
                            let ni = nr as usize * w + nc as usize;
                            if obs.owners[ni] == Some(obs.player)
                                && consolidation_dist[ni] < best_cd
                            {
                                best_cd = consolidation_dist[ni];
                                best_cd_dir = Some(dir);
                            }
                        }
                        if let Some(dir) = best_cd_dir {
                            orders.push(Order {
                                row,
                                col,
                                dir,
                                priority: my_army as f32 * 0.3 - 200.0,
                            });
                        }
                    }
                } else {
                    // === INTERIOR CELL ===
                    // Every cell with army >= 2 consolidates toward objective.
                    if my_army < 2 {
                        continue;
                    }

                    let my_cd = consolidation_dist[idx];
                    let mut best_dir = None;
                    let mut best_cd = my_cd;

                    for dir in Direction::ALL {
                        let (dr, dc) = dir.delta();
                        let nr = row as i32 + dr;
                        let nc = col as i32 + dc;
                        if nr < 0 || nc < 0 || (nr as usize) >= h || (nc as usize) >= w {
                            continue;
                        }
                        let ni = nr as usize * w + nc as usize;
                        if obs.owners[ni] != Some(obs.player) {
                            continue;
                        }
                        if consolidation_dist[ni] < best_cd {
                            best_cd = consolidation_dist[ni];
                            best_dir = Some(dir);
                        }
                    }

                    if let Some(dir) = best_dir {
                        let priority = if defense_mode.is_some() {
                            my_army as f32 * 2.0
                        } else if marauder_dist[idx] < intercept_radius {
                            // Intercept priority: high, scales with army.
                            my_army as f32 * 1.5
                        } else {
                            my_army as f32 * 0.5 - 100.0
                        };
                        orders.push(Order {
                            row,
                            col,
                            dir,
                            priority,
                        });
                    }
                }
            }
        }

        orders.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        orders
            .into_iter()
            .map(|o| Action::Move {
                row: o.row,
                col: o.col,
                dir: o.dir,
                split: false,
            })
            .collect()
    }
}

impl Agent for PressureAgent {
    fn name(&self) -> &str {
        "pressure"
    }

    fn version(&self) -> &str {
        "v3"
    }

    fn act(&mut self, obs: &Observation, rng: &mut dyn RngCore) -> Vec<Action> {
        self.update_memory(obs);
        self.build_player_models(obs);
        self.emit_orders(obs, rng)
    }

    fn reset(&mut self) {
        self.player_models.clear();
        self.memory_owner.clear();
        self.memory_army.clear();
        self.memory_turn.clear();
        self.own_general = None;
        self.enemy_generals.clear();
        self.initialized = false;
    }
}
