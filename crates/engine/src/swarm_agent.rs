use std::collections::VecDeque;

use rand::RngCore;

use crate::action::{Action, Direction};
use crate::agent::{Agent, Observation, valid_dirs};
use crate::state::Tile;

/// Marching-column agent: maximum force concentration.
///
/// Two modes:
/// - **Expand** (pre-contact): Directional expansion toward map center.
/// - **March** (post-contact): Find the biggest owned stack. March it straight
///   toward the enemy general/centroid. Everything else in the empire consolidates
///   toward the march head, creating a continuous pipeline of reinforcements.
///
/// The march head is a single cell with massive army that captures everything
/// in its path. Armies flow from the interior along the trail it leaves behind,
/// feeding it more troops each turn. This creates the maximum possible local
/// force superiority at the point of attack.
///
/// Unlike pressure (which spreads across 25% of frontier) or the old rally
/// mechanic (which spread across a border area), the march puts ALL force
/// into ONE cell — the tip of the spear.
pub struct SwarmAgent {
    memory_owner: Vec<Option<u8>>,
    memory_army: Vec<i32>,
    memory_turn: Vec<u32>,
    own_general: Option<usize>,
    enemy_generals: Vec<Option<usize>>,
    initialized: bool,
}

impl SwarmAgent {
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

    /// Find the best attack target: known enemy general, or centroid of visible enemy.
    fn best_attack_target(&self, obs: &Observation) -> Option<usize> {
        for entry in &self.enemy_generals {
            if let Some(idx) = entry {
                if self.memory_owner[*idx].is_some_and(|o| o != obs.player) {
                    return Some(*idx);
                }
            }
        }
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

    /// Find the march head: the owned cell best suited for the attack.
    /// Balances army size with proximity to the target — a big stack near
    /// the enemy is better than a bigger stack deep in the interior.
    fn find_march_head(&self, obs: &Observation, target_dist: &[u32]) -> Option<usize> {
        let n = obs.width * obs.height;
        let mut best_idx = None;
        let mut best_score = i64::MIN;
        for i in 0..n {
            if obs.owners[i] != Some(obs.player) || obs.armies[i] <= 3 {
                continue;
            }
            // Don't pick the general unless it has huge army.
            if Some(i) == self.own_general && obs.armies[i] < 50 {
                continue;
            }
            let td = target_dist[i];
            if td == u32::MAX {
                continue;
            }
            // Score: army matters most, but proximity to target is a tiebreaker.
            let score = obs.armies[i] as i64 * 10 - td as i64 * 3;
            if score > best_score {
                best_score = score;
                best_idx = Some(i);
            }
        }
        best_idx
    }

    fn emit_expand_orders(&self, obs: &Observation) -> Vec<Action> {
        let w = obs.width;
        let h = obs.height;
        let center_r = h as f32 / 2.0;
        let center_c = w as f32 / 2.0;

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

                if !is_frontier {
                    if my_army < 3 {
                        continue;
                    }
                    if let Some(dir) = dirs
                        .iter()
                        .copied()
                        .filter(|&dir| {
                            let (dr, dc) = dir.delta();
                            let nr = (row as i32 + dr) as usize;
                            let nc = (col as i32 + dc) as usize;
                            let ni = nr * w + nc;
                            obs.owners[ni] == Some(obs.player) && frontier_dist[ni] < my_fd
                        })
                        .min_by_key(|&dir| {
                            let (dr, dc) = dir.delta();
                            let nr = (row as i32 + dr) as usize;
                            let nc = (col as i32 + dc) as usize;
                            frontier_dist[nr * w + nc]
                        })
                    {
                        orders.push(Action::Move {
                            row,
                            col,
                            dir,
                            split: false,
                        });
                    }
                    continue;
                }

                let mut best_score = i32::MIN;
                let mut best_dir = None;

                for &dir in &dirs {
                    let (dr, dc) = dir.delta();
                    let nr = (row as i32 + dr) as usize;
                    let nc = (col as i32 + dc) as usize;

                    if obs.owners[nr * w + nc] == Some(obs.player) {
                        continue;
                    }
                    let target_army = obs.armies[nr * w + nc];
                    if my_army - 1 <= target_army {
                        continue;
                    }

                    let target_tile = obs.tiles[nr * w + nc];
                    let mut score: i32 = if target_tile == Tile::City { 2000 } else { 1000 };
                    let dist_to_center =
                        ((nr as f32 - center_r).abs() + (nc as f32 - center_c).abs()) as i32;
                    score -= dist_to_center * 10;

                    if score > best_score {
                        best_score = score;
                        best_dir = Some(dir);
                    }
                }

                if let Some(dir) = best_dir {
                    orders.push(Action::Move {
                        row,
                        col,
                        dir,
                        split: false,
                    });
                }
            }
        }

        orders
    }

    fn emit_march_orders(&self, obs: &Observation) -> Vec<Action> {
        let w = obs.width;
        let h = obs.height;

        let target = self.best_attack_target(obs);

        // BFS from target through all passable terrain.
        let target_dist = match target {
            Some(t) => Self::bfs_multi(obs, &[t]),
            None => return self.emit_expand_orders(obs),
        };

        let march_head = match self.find_march_head(obs, &target_dist) {
            Some(idx) => idx,
            None => return self.emit_expand_orders(obs),
        };

        // Build frontier for non-march cells.
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

        // Consolidation: all interior armies flow toward the march head.
        let march_dist = Self::bfs_owned(obs, &[march_head]);

        let mut orders = Vec::new();

        let march_row = march_head / w;
        let march_col = march_head % w;

        // === MARCH HEAD: move toward target ===
        if obs.armies[march_head] > 1 {
            let my_army = obs.armies[march_head];
            let dirs = valid_dirs(obs, march_row, march_col);

            let mut best_score = i32::MIN;
            let mut best_dir = None;

            for &dir in &dirs {
                let (dr, dc) = dir.delta();
                let nr = (march_row as i32 + dr) as usize;
                let nc = (march_col as i32 + dc) as usize;
                let ni = nr * w + nc;

                let dest_army = obs.armies[ni];
                let dest_tile = obs.tiles[ni];
                let is_own = obs.owners[ni] == Some(obs.player);
                let is_enemy = obs.owners[ni].is_some_and(|o| o != obs.player);
                let can_capture = my_army - 1 > dest_army;

                let score = if dest_tile == Tile::General && is_enemy && can_capture {
                    100_000
                } else if is_own {
                    // Moving through own territory toward target.
                    let td = target_dist[ni];
                    let my_td = target_dist[march_head];
                    if td < my_td {
                        // Getting closer — good.
                        500 - td as i32
                    } else {
                        -1000
                    }
                } else if can_capture {
                    // Can capture: prefer cells closer to target.
                    let td = target_dist[ni];
                    let base = if is_enemy { 2000 } else if dest_tile == Tile::City { 3000 } else { 1000 };
                    base - td as i32 * 5
                } else {
                    // Can't capture: maybe retreat through own territory.
                    -500
                };

                if score > best_score {
                    best_score = score;
                    best_dir = Some(dir);
                }
            }

            if let Some(dir) = best_dir {
                if best_score > -500 {
                    orders.push(Action::Move {
                        row: march_row,
                        col: march_col,
                        dir,
                        split: false,
                    });
                }
            }
        }

        // === ALL OTHER CELLS ===
        for row in 0..h {
            for col in 0..w {
                let idx = row * w + col;
                if idx == march_head {
                    continue;
                }
                if obs.owners[idx] != Some(obs.player) || obs.armies[idx] <= 1 {
                    continue;
                }

                let my_army = obs.armies[idx];
                let my_fd = frontier_dist[idx];
                let is_frontier = my_fd == 0;
                let my_md = march_dist[idx];
                let dirs = valid_dirs(obs, row, col);
                if dirs.is_empty() {
                    continue;
                }

                if is_frontier {
                    // Frontier cells: try to expand (grab land/cities for economy).
                    // If nothing to grab, consolidate toward march head.
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
                            3000
                        } else if is_enemy && can_capture {
                            1500
                        } else if !is_enemy && can_capture {
                            1000
                        } else {
                            -100
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

                    // Frontier cells that can't expand hold position — don't
                    // drain them toward the march. Maintaining frontier defense
                    // and expansion rate is more important than feeding the march.
                } else {
                    // Interior: consolidate toward march head.
                    if my_army < 2 {
                        continue;
                    }

                    if let Some(dir) = dirs
                        .iter()
                        .copied()
                        .filter(|&dir| {
                            let (dr, dc) = dir.delta();
                            let nr = (row as i32 + dr) as usize;
                            let nc = (col as i32 + dc) as usize;
                            let ni = nr * w + nc;
                            obs.owners[ni] == Some(obs.player) && march_dist[ni] < my_md
                        })
                        .min_by_key(|&dir| {
                            let (dr, dc) = dir.delta();
                            let nr = (row as i32 + dr) as usize;
                            let nc = (col as i32 + dc) as usize;
                            march_dist[nr * w + nc]
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

impl Agent for SwarmAgent {
    fn name(&self) -> &str {
        "swarm"
    }

    fn version(&self) -> &str {
        "v3"
    }

    fn act(&mut self, obs: &Observation, _rng: &mut dyn RngCore) -> Vec<Action> {
        self.update_memory(obs);

        let n = obs.width * obs.height;
        let has_enemy =
            (0..n).any(|i| obs.visible[i] && obs.owners[i].is_some_and(|o| o != obs.player));

        if has_enemy {
            self.emit_march_orders(obs)
        } else {
            self.emit_expand_orders(obs)
        }
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
