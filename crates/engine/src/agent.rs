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
    fn version(&self) -> &str {
        "v1"
    }
    /// Display identity for scoreboards: "name-version".
    fn id(&self) -> String {
        format!("{}-{}", self.name(), self.version())
    }
    fn act(&mut self, obs: &Observation, rng: &mut dyn RngCore) -> Vec<Action>;
    fn reset(&mut self) {}
}

/// Returns all built-in agents (one instance of each).
/// If SIMEV_PYTHON_CLIENT is set, also includes the Python graph search agent.
pub fn all_builtin_agents() -> Vec<Box<dyn Agent>> {
    let mut agents: Vec<Box<dyn Agent>> = vec![
        Box::new(crate::expander_agent::ExpanderAgent::new()),
        Box::new(crate::swarm_agent::SwarmAgent::new()),
        Box::new(crate::pressure_agent::PressureAgent::new()),
    ];

    if let Ok(client_dir) = std::env::var("SIMEV_PYTHON_CLIENT") {
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
        "expander" => Some(Box::new(crate::expander_agent::ExpanderAgent::new())),
        "swarm" => Some(Box::new(crate::swarm_agent::SwarmAgent::new())),
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
        Box::new(crate::expander_agent::ExpanderAgent::new()),
        Box::new(crate::swarm_agent::SwarmAgent::new()),
        Box::new(crate::pressure_agent::PressureAgent::new()),
    ]
}

/// Helpers shared across agents.
pub(crate) fn valid_dirs(obs: &Observation, row: usize, col: usize) -> Vec<Direction> {
    let mut dirs = Vec::new();
    for dir in Direction::ALL {
        let (dr, dc) = dir.delta();
        let nr = row as i32 + dr;
        let nc = col as i32 + dc;
        if nr >= 0
            && nc >= 0
            && (nr as usize) < obs.height
            && (nc as usize) < obs.width
            && obs.tile(nr as usize, nc as usize) != Tile::Mountain
        {
            dirs.push(dir);
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
}

// ExpanderAgent and SwarmAgent are now in their own modules:
// - crate::expander_agent::ExpanderAgent
// - crate::swarm_agent::SwarmAgent
