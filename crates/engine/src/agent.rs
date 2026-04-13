use crate::action::{Action, Direction};
use crate::state::{GameState, Tile};
use rand::{Rng, RngCore};
use serde::Serialize;

/// Observation visible to a single player (fog of war applied).
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub width: usize,
    pub height: usize,
    pub player: u8,
    pub turn: u32,
    /// Per-cell visibility. true = visible (within 1 tile of owned cell).
    pub visible: Vec<bool>,
    /// What the player can see. Non-visible cells show Tile::Empty, armies=0, owner=None.
    pub tiles: Vec<Tile>,
    pub armies: Vec<i32>,
    pub owners: Vec<Option<u8>>,
    /// Own stats.
    pub my_land: usize,
    pub my_armies: i32,
    /// Per-opponent stats (visible totals only for land; army total is global knowledge like the original game).
    pub opponent_stats: Vec<(u8, usize, i32)>,
}

impl Observation {
    pub fn from_state(state: &GameState, player: u8) -> Self {
        let n = state.width * state.height;
        let mut visible = vec![false; n];

        // Mark cells within 1 tile of any owned cell as visible.
        for row in 0..state.height {
            for col in 0..state.width {
                if state.cell(row, col).owner == Some(player) {
                    for dr in -1i32..=1 {
                        for dc in -1i32..=1 {
                            let r = row as i32 + dr;
                            let c = col as i32 + dc;
                            if state.in_bounds(r, c) {
                                visible[r as usize * state.width + c as usize] = true;
                            }
                        }
                    }
                }
            }
        }

        let mut tiles = vec![Tile::Empty; n];
        let mut armies = vec![0i32; n];
        let mut owners: Vec<Option<u8>> = vec![None; n];

        for i in 0..n {
            if visible[i] {
                tiles[i] = state.grid[i].tile;
                armies[i] = state.grid[i].armies;
                owners[i] = state.grid[i].owner;
            } else {
                // Mountains and cities are visible as structures even in fog.
                match state.grid[i].tile {
                    Tile::Mountain => tiles[i] = Tile::Mountain,
                    Tile::City => tiles[i] = Tile::City,
                    _ => {}
                }
            }
        }

        let my_land = state.land_count(player);
        let my_armies = state.army_count(player);

        let mut opponent_stats = Vec::new();
        for p in 0..state.num_players {
            if p != player {
                opponent_stats.push((p, state.land_count(p), state.army_count(p)));
            }
        }

        Observation {
            width: state.width,
            height: state.height,
            player,
            turn: state.turn,
            visible,
            tiles,
            armies,
            owners,
            my_land,
            my_armies,
            opponent_stats,
        }
    }

    pub fn tile(&self, row: usize, col: usize) -> Tile {
        self.tiles[row * self.width + col]
    }

    pub fn army(&self, row: usize, col: usize) -> i32 {
        self.armies[row * self.width + col]
    }

    pub fn owner(&self, row: usize, col: usize) -> Option<u8> {
        self.owners[row * self.width + col]
    }

    pub fn is_visible(&self, row: usize, col: usize) -> bool {
        self.visible[row * self.width + col]
    }
}

/// Agent interface. Implement this to create a bot.
/// Returns a Vec of actions — any number of orders per turn.
/// Orders are executed sequentially within a turn for each player.
pub trait Agent: Send {
    fn name(&self) -> &str;
    fn version(&self) -> &str { "v1" }
    /// Display identity for scoreboards: "name-version".
    fn id(&self) -> String {
        format!("{}-{}", self.name(), self.version())
    }
    fn act(&mut self, obs: &Observation, rng: &mut dyn RngCore) -> Vec<Action>;
    fn reset(&mut self) {}
}

/// Returns all built-in agents (one instance of each).
/// If GENERALS_PYTHON_CLIENT is set, also includes the Python graph search agent.
pub fn all_builtin_agents() -> Vec<Box<dyn Agent>> {
    let mut agents: Vec<Box<dyn Agent>> = vec![
        Box::new(ExpanderAgent),
        Box::new(SwarmAgent),
        Box::new(crate::pressure_agent::PressureAgent::new()),
    ];

    if let Ok(client_dir) = std::env::var("GENERALS_PYTHON_CLIENT") {
        agents.push(Box::new(crate::subprocess_agent::SubprocessAgent::new(
            "graph-search",
            "python3",
            vec![format!("{}/stdio_agent.py", client_dir)],
        )));
    }

    agents
}

/// Look up a single agent by name (e.g. "pressure", "swarm", "expander").
/// Returns None if the name doesn't match any built-in agent.
pub fn agent_by_name(name: &str) -> Option<Box<dyn Agent>> {
    match name {
        "expander" => Some(Box::new(ExpanderAgent)),
        "swarm" => Some(Box::new(SwarmAgent)),
        "pressure" => Some(Box::new(crate::pressure_agent::PressureAgent::new())),
        "random" => Some(Box::new(RandomAgent)),
        _ => None,
    }
}

/// Returns the list of known built-in agent names.
pub fn builtin_agent_names() -> &'static [&'static str] {
    &["expander", "swarm", "pressure", "random"]
}

/// Agents eligible for round-robin.
pub fn rr_agents() -> Vec<Box<dyn Agent>> {
    vec![
        Box::new(ExpanderAgent),
        Box::new(SwarmAgent),
        Box::new(crate::pressure_agent::PressureAgent::new()),
    ]
}

/// Helpers shared across agents.
fn valid_dirs(obs: &Observation, row: usize, col: usize) -> Vec<Direction> {
    let mut dirs = Vec::new();
    for dir in Direction::ALL {
        let (dr, dc) = dir.delta();
        let nr = row as i32 + dr;
        let nc = col as i32 + dc;
        if nr >= 0 && nc >= 0 && (nr as usize) < obs.height && (nc as usize) < obs.width {
            if obs.tile(nr as usize, nc as usize) != Tile::Mountain {
                dirs.push(dir);
            }
        }
    }
    dirs
}

/// Random agent: moves every cell with >1 army in a random valid direction.
pub struct RandomAgent;

impl Agent for RandomAgent {
    fn name(&self) -> &str {
        "random"
    }

    fn act(&mut self, obs: &Observation, rng: &mut dyn RngCore) -> Vec<Action> {
        let mut orders = Vec::new();
        for row in 0..obs.height {
            for col in 0..obs.width {
                if obs.owner(row, col) != Some(obs.player) || obs.army(row, col) <= 1 {
                    continue;
                }
                let dirs = valid_dirs(obs, row, col);
                if !dirs.is_empty() {
                    let dir = dirs[rng.gen_range(0..dirs.len())];
                    orders.push(Action::Move { row, col, dir, split: false });
                }
            }
        }
        orders
    }
}

/// Expander agent: every cell with >1 army issues an order.
/// Frontier cells expand outward. Interior cells consolidate toward the nearest frontier.
pub struct ExpanderAgent;

impl ExpanderAgent {
    /// BFS distance from every cell to the nearest frontier (non-owned neighbor).
    fn frontier_distance(obs: &Observation) -> Vec<u32> {
        use std::collections::VecDeque;

        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();

        // Seed: owned cells adjacent to a non-owned, non-mountain cell.
        for row in 0..obs.height {
            for col in 0..obs.width {
                if obs.owner(row, col) != Some(obs.player) {
                    continue;
                }
                let is_frontier = valid_dirs(obs, row, col).iter().any(|dir| {
                    let (dr, dc) = dir.delta();
                    let nr = (row as i32 + dr) as usize;
                    let nc = (col as i32 + dc) as usize;
                    obs.owner(nr, nc) != Some(obs.player)
                });
                if is_frontier {
                    let idx = row * obs.width + col;
                    dist[idx] = 0;
                    queue.push_back((row, col));
                }
            }
        }

        // BFS inward through owned territory.
        while let Some((r, c)) = queue.pop_front() {
            let d = dist[r * obs.width + c];
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr < 0 || nc < 0 || (nr as usize) >= obs.height || (nc as usize) >= obs.width {
                    continue;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                let ni = nr * obs.width + nc;
                if obs.owner(nr, nc) == Some(obs.player) && dist[ni] > d + 1 {
                    dist[ni] = d + 1;
                    queue.push_back((nr, nc));
                }
            }
        }

        dist
    }
}

impl Agent for ExpanderAgent {
    fn name(&self) -> &str {
        "expander"
    }

    fn act(&mut self, obs: &Observation, _rng: &mut dyn RngCore) -> Vec<Action> {
        let frontier_dist = Self::frontier_distance(obs);
        let mut orders = Vec::new();

        for row in 0..obs.height {
            for col in 0..obs.width {
                if obs.owner(row, col) != Some(obs.player) || obs.army(row, col) <= 1 {
                    continue;
                }

                let my_army = obs.army(row, col);
                let my_fd = frontier_dist[row * obs.width + col];
                let dirs = valid_dirs(obs, row, col);
                if dirs.is_empty() {
                    continue;
                }

                let is_frontier = my_fd == 0;

                // Interior cells: only move if army >= 3 (don't drain 2s into useless 1s).
                if !is_frontier && my_army < 3 {
                    continue;
                }

                // Score each direction.
                let mut best_score = i32::MIN;
                let mut best_dir = dirs[0];

                for &dir in &dirs {
                    let (dr, dc) = dir.delta();
                    let nr = (row as i32 + dr) as usize;
                    let nc = (col as i32 + dc) as usize;

                    let target_owner = obs.owner(nr, nc);
                    let target_army = obs.army(nr, nc);
                    let target_tile = obs.tile(nr, nc);

                    // Massive bonus for capturing generals.
                    if target_tile == Tile::General && target_owner != Some(obs.player) {
                        if my_army - 1 > target_army {
                            best_score = 10000;
                            best_dir = dir;
                            continue;
                        }
                    }

                    let score = if target_owner == Some(obs.player) {
                        // Own cell: consolidate toward frontier.
                        let target_fd = frontier_dist[nr * obs.width + nc];
                        if target_fd < my_fd {
                            -100 // Moving toward frontier.
                        } else {
                            -1000 // Moving away — skip.
                        }
                    } else if target_owner.is_none() {
                        // Unowned.
                        if target_tile == Tile::City {
                            // Cities: high value if we can take them.
                            if my_army - 1 > target_army {
                                1500
                            } else {
                                -200
                            }
                        } else if my_army - 1 > target_army {
                            // Empty cell we can capture.
                            1000
                        } else {
                            -200
                        }
                    } else {
                        // Enemy cell.
                        if my_army - 1 > target_army {
                            800
                        } else {
                            -200
                        }
                    };

                    if score > best_score {
                        best_score = score;
                        best_dir = dir;
                    }
                }

                if best_score > -200 {
                    orders.push(Action::Move {
                        row,
                        col,
                        dir: best_dir,
                        split: false,
                    });
                }
            }
        }

        orders
    }
}

/// Swarm agent: converges all armies toward the nearest enemy cell.
/// Where the expander spreads outward across empty space, the swarm
/// ignores unclaimed territory and rushes toward enemy positions.
/// Every cell flows along shortest-path to the nearest visible enemy,
/// creating converging pressure from all angles.
pub struct SwarmAgent;

impl SwarmAgent {
    /// BFS distance from every cell to the nearest visible enemy cell.
    fn enemy_distance(obs: &Observation) -> Vec<u32> {
        use std::collections::VecDeque;

        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();

        for row in 0..obs.height {
            for col in 0..obs.width {
                if let Some(o) = obs.owner(row, col) {
                    if o != obs.player {
                        let idx = row * obs.width + col;
                        dist[idx] = 0;
                        queue.push_back((row, col));
                    }
                }
            }
        }

        while let Some((r, c)) = queue.pop_front() {
            let d = dist[r * obs.width + c];
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr < 0 || nc < 0 || (nr as usize) >= obs.height || (nc as usize) >= obs.width {
                    continue;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                if obs.tile(nr, nc) == Tile::Mountain {
                    continue;
                }
                let ni = nr * obs.width + nc;
                if dist[ni] > d + 1 {
                    dist[ni] = d + 1;
                    queue.push_back((nr, nc));
                }
            }
        }

        dist
    }

    /// BFS distance from every owned cell to the nearest frontier (non-owned neighbor).
    /// Used for early-game directional expansion: interior cells consolidate
    /// toward the frontier, frontier cells expand outward toward the map center
    /// (where the enemy likely is).
    fn frontier_distance(obs: &Observation) -> Vec<u32> {
        use std::collections::VecDeque;

        let n = obs.width * obs.height;
        let mut dist = vec![u32::MAX; n];
        let mut queue = VecDeque::new();

        for row in 0..obs.height {
            for col in 0..obs.width {
                if obs.owner(row, col) != Some(obs.player) {
                    continue;
                }
                let is_frontier = valid_dirs(obs, row, col).iter().any(|dir| {
                    let (dr, dc) = dir.delta();
                    let nr = (row as i32 + dr) as usize;
                    let nc = (col as i32 + dc) as usize;
                    obs.owner(nr, nc) != Some(obs.player)
                });
                if is_frontier {
                    let idx = row * obs.width + col;
                    dist[idx] = 0;
                    queue.push_back((row, col));
                }
            }
        }

        while let Some((r, c)) = queue.pop_front() {
            let d = dist[r * obs.width + c];
            for dir in Direction::ALL {
                let (dr, dc) = dir.delta();
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr < 0 || nc < 0 || (nr as usize) >= obs.height || (nc as usize) >= obs.width {
                    continue;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                let ni = nr * obs.width + nc;
                if obs.owner(nr, nc) == Some(obs.player) && dist[ni] > d + 1 {
                    dist[ni] = d + 1;
                    queue.push_back((nr, nc));
                }
            }
        }

        dist
    }
}

impl Agent for SwarmAgent {
    fn name(&self) -> &str {
        "swarm"
    }
    fn version(&self) -> &str { "v2" }

    fn act(&mut self, obs: &Observation, _rng: &mut dyn RngCore) -> Vec<Action> {
        let enemy_dist = Self::enemy_distance(obs);
        let has_enemy_visible = enemy_dist.iter().any(|&d| d == 0);
        let frontier_dist = Self::frontier_distance(obs);

        // Map center — used to bias early expansion toward the enemy's likely position.
        let center_r = obs.height as f32 / 2.0;
        let center_c = obs.width as f32 / 2.0;

        let mut orders = Vec::new();

        for row in 0..obs.height {
            for col in 0..obs.width {
                if obs.owner(row, col) != Some(obs.player) || obs.army(row, col) <= 1 {
                    continue;
                }

                let my_army = obs.army(row, col);
                let my_ed = enemy_dist[row * obs.width + col];
                let my_fd = frontier_dist[row * obs.width + col];
                let dirs = valid_dirs(obs, row, col);
                if dirs.is_empty() {
                    continue;
                }

                // === FIX 1: Directional early expansion ===
                // Before enemy contact, use expander-style frontier BFS but bias
                // frontier cells toward the map center (where the enemy likely is).
                if !has_enemy_visible {
                    let is_frontier = my_fd == 0;

                    // Interior cells: consolidate toward frontier, skip if < 3.
                    if !is_frontier {
                        if my_army < 3 {
                            continue;
                        }
                        // Move toward lower frontier_dist.
                        let best = dirs.iter().copied()
                            .filter(|&dir| {
                                let (dr, dc) = dir.delta();
                                let nr = (row as i32 + dr) as usize;
                                let nc = (col as i32 + dc) as usize;
                                let ni = nr * obs.width + nc;
                                obs.owner(nr, nc) == Some(obs.player)
                                    && frontier_dist[ni] < my_fd
                            })
                            .min_by_key(|&dir| {
                                let (dr, dc) = dir.delta();
                                let nr = (row as i32 + dr) as usize;
                                let nc = (col as i32 + dc) as usize;
                                frontier_dist[nr * obs.width + nc]
                            });
                        if let Some(dir) = best {
                            orders.push(Action::Move { row, col, dir, split: false });
                        }
                        continue;
                    }

                    // Frontier cell: score each unowned neighbor, biased toward center.
                    let mut best_score = i32::MIN;
                    let mut best_dir = None;

                    for &dir in &dirs {
                        let (dr, dc) = dir.delta();
                        let nr = (row as i32 + dr) as usize;
                        let nc = (col as i32 + dc) as usize;

                        if obs.owner(nr, nc) == Some(obs.player) {
                            continue;
                        }
                        let target_army = obs.army(nr, nc);
                        if my_army - 1 <= target_army {
                            continue;
                        }

                        let target_tile = obs.tile(nr, nc);

                        // City bonus.
                        let mut score: i32 = if target_tile == Tile::City {
                            1500
                        } else {
                            1000
                        };

                        // Bias toward map center (closer to center = higher score).
                        let dist_to_center = ((nr as f32 - center_r).abs()
                            + (nc as f32 - center_c).abs()) as i32;
                        score -= dist_to_center * 10;

                        if score > best_score {
                            best_score = score;
                            best_dir = Some(dir);
                        }
                    }

                    if let Some(dir) = best_dir {
                        orders.push(Action::Move { row, col, dir, split: false });
                    }
                    continue;
                }

                // === FIX 2: Consolidation threshold on the contact line ===
                // Near the enemy (ed <= 3): require >= 3 armies before moving,
                // so we don't dribble 1s that get instantly recaptured.
                // Deep interior (ed > 3): also require >= 3 (unchanged).
                if my_army < 3 {
                    continue;
                }

                let mut best_score = i32::MIN;
                let mut best_dir = dirs[0];

                for &dir in &dirs {
                    let (dr, dc) = dir.delta();
                    let nr = (row as i32 + dr) as usize;
                    let nc = (col as i32 + dc) as usize;

                    let target_owner = obs.owner(nr, nc);
                    let target_army = obs.army(nr, nc);
                    let target_tile = obs.tile(nr, nc);
                    let target_ed = enemy_dist[nr * obs.width + nc];

                    // Generals are the ultimate target.
                    if target_tile == Tile::General && target_owner != Some(obs.player) {
                        if my_army - 1 > target_army {
                            best_score = 10000;
                            best_dir = dir;
                            continue;
                        }
                    }

                    let score = if let Some(o) = target_owner {
                        if o == obs.player {
                            // Own cell: move toward enemy.
                            if target_ed < my_ed {
                                -50
                            } else {
                                -1000
                            }
                        } else {
                            // Enemy cell: attack!
                            if my_army - 1 > target_army {
                                2000 - target_army
                            } else if my_army * 2 > target_army {
                                100
                            } else {
                                -300
                            }
                        }
                    } else {
                        // === FIX 3: Opportunistic economy ===
                        // Cities are always worth grabbing if affordable.
                        if target_tile == Tile::City && my_army - 1 > target_army {
                            1500
                        } else if target_ed < my_ed {
                            // Moving through unowned toward enemy — capture on the way.
                            if my_army - 1 > target_army {
                                500
                            } else {
                                -200
                            }
                        } else if my_ed > 5 && my_army - 1 > target_army {
                            // Far from enemy: grab free land to build economy.
                            // Weaker than moving toward enemy, but better than sitting idle.
                            200
                        } else {
                            -500
                        }
                    };

                    if score > best_score {
                        best_score = score;
                        best_dir = dir;
                    }
                }

                if best_score > -200 {
                    orders.push(Action::Move {
                        row,
                        col,
                        dir: best_dir,
                        split: false,
                    });
                }
            }
        }

        orders
    }
}
