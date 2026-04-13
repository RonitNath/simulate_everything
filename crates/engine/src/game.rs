use crate::action::Action;
use crate::agent::Observation;
use crate::event::{Event, PlayerAction, PlayerStats};
use crate::state::{GameState, Tile};
use rand::Rng;
use rand::rngs::StdRng;
use rand::SeedableRng;

/// Runs the game loop. Manages turn execution, army growth, and win conditions.
pub struct Game {
    pub state: GameState,
    pub max_turns: u32,
    pub events: Vec<Event>,
    rng: StdRng,
}

impl Game {
    pub fn new(state: GameState, max_turns: u32) -> Self {
        Self::with_seed(state, max_turns, 0)
    }

    pub fn with_seed(state: GameState, max_turns: u32, seed: u64) -> Self {
        let start_event = Event::GameStart {
            width: state.width,
            height: state.height,
            num_players: state.num_players,
            turn: 0,
        };
        Self {
            state,
            max_turns,
            events: vec![start_event],
            rng: StdRng::seed_from_u64(seed),
        }
    }

    pub fn is_over(&self) -> bool {
        self.state.winner.is_some() || self.state.turn >= self.max_turns
    }

    pub fn observations(&self) -> Vec<Observation> {
        (0..self.state.num_players)
            .map(|p| Observation::from_state(&self.state, p))
            .collect()
    }

    /// Execute one turn. Each player submits a Vec of actions (multiple orders per turn).
    /// Actions are interleaved round-robin: player0[0], player1[0], player0[1], player1[1], ...
    pub fn step(&mut self, player_orders: &[(u8, Vec<Action>)]) {
        let mut player_actions = Vec::new();
        let mut executed_counts: Vec<usize> = vec![0; player_orders.len()];

        // Find max number of orders any player submitted.
        let max_orders = player_orders.iter().map(|(_, acts)| acts.len()).max().unwrap_or(0);

        // Interleave execution round-robin.
        for order_idx in 0..max_orders {
            for (pi, (player, actions)) in player_orders.iter().enumerate() {
                if order_idx >= actions.len() {
                    continue;
                }
                if !self.state.alive[*player as usize] {
                    continue;
                }
                let action = actions[order_idx];
                if self.execute_action(*player, action) {
                    executed_counts[pi] += 1;
                }
            }
        }

        // Build event records.
        for (pi, (player, actions)) in player_orders.iter().enumerate() {
            player_actions.push(PlayerAction {
                player: *player,
                actions: actions.clone(),
                executed: executed_counts[pi],
            });
        }

        // Army growth.
        self.grow_armies();

        self.state.turn += 1;

        // Check for winner.
        self.check_winner();

        let stats = self.player_stats();
        self.events.push(Event::Turn {
            turn: self.state.turn,
            actions: player_actions,
            stats: stats.clone(),
        });

        if self.is_over() {
            self.events.push(Event::GameEnd {
                turn: self.state.turn,
                winner: self.state.winner,
                stats,
            });
        }
    }

    fn execute_action(&mut self, player: u8, action: Action) -> bool {
        let Action::Move {
            row,
            col,
            dir,
            split,
        } = action
        else {
            return true;
        };

        let s = &self.state;
        if row >= s.height || col >= s.width {
            return false;
        }

        let src = s.cell(row, col);
        if src.owner != Some(player) || src.armies <= 1 {
            return false;
        }

        let (dr, dc) = dir.delta();
        let nr = row as i32 + dr;
        let nc = col as i32 + dc;

        if !s.in_bounds(nr, nc) {
            return false;
        }
        let (nr, nc) = (nr as usize, nc as usize);

        if s.cell(nr, nc).tile == Tile::Mountain {
            return false;
        }

        let src_armies = self.state.cell(row, col).armies;
        let send = if split {
            src_armies / 2
        } else {
            src_armies - 1
        };

        if send <= 0 {
            return false;
        }

        self.state.cell_mut(row, col).armies -= send;

        let dst = self.state.cell(nr, nc);
        let dst_owner = dst.owner;
        let dst_armies = dst.armies;

        match dst_owner {
            Some(owner) if owner == player => {
                self.state.cell_mut(nr, nc).armies += send;
            }
            Some(defender) => {
                if send > dst_armies {
                    let remaining = send - dst_armies;
                    let dst_cell = self.state.cell_mut(nr, nc);
                    dst_cell.armies = remaining;
                    dst_cell.owner = Some(player);

                    if dst_cell.tile == Tile::General {
                        self.eliminate(defender, player);
                    }
                } else {
                    self.state.cell_mut(nr, nc).armies = dst_armies - send;
                }
            }
            None => {
                if dst.tile == Tile::City {
                    if send > dst_armies {
                        let dst_cell = self.state.cell_mut(nr, nc);
                        dst_cell.armies = send - dst_armies;
                        dst_cell.owner = Some(player);
                    } else {
                        self.state.cell_mut(nr, nc).armies = dst_armies - send;
                    }
                } else {
                    let dst_cell = self.state.cell_mut(nr, nc);
                    dst_cell.armies = send;
                    dst_cell.owner = Some(player);
                }
            }
        }

        true
    }

    fn eliminate(&mut self, loser: u8, winner: u8) {
        self.state.alive[loser as usize] = false;

        for cell in &mut self.state.grid {
            if cell.owner == Some(loser) {
                cell.owner = Some(winner);
            }
        }

        let (gr, gc) = self.state.general_positions[loser as usize];
        self.state.cell_mut(gr, gc).tile = Tile::City;

        self.events.push(Event::Elimination {
            turn: self.state.turn,
            eliminated: loser,
            by: winner,
        });
    }

    fn grow_armies(&mut self) {
        // Reinforcement wave: every WAVE_INTERVAL turns, all owned land gets +1
        // and structures get a bonus. Between waves, only structures grow (+1/turn).
        const WAVE_INTERVAL: u32 = 25;
        let is_wave = (self.state.turn + 1) % WAVE_INTERVAL == 0;

        for cell in &mut self.state.grid {
            if cell.owner.is_none() {
                continue;
            }
            match cell.tile {
                // Generals and cities: +1 every turn.
                // On wave turns, cities get +2 extra (total +3), generals +1 extra (total +2).
                Tile::City => {
                    cell.armies += 1;
                    if is_wave {
                        cell.armies += 2;
                    }
                }
                Tile::General => {
                    cell.armies += 1;
                    if is_wave {
                        cell.armies += 1;
                    }
                }
                // Empty owned land: only grows on wave turns (+1).
                Tile::Empty => {
                    if is_wave {
                        cell.armies += 1;
                    }
                }
                Tile::Mountain => {}
            }
        }
    }

    fn check_winner(&mut self) {
        let alive: Vec<u8> = (0..self.state.num_players)
            .filter(|&p| self.state.alive[p as usize])
            .collect();

        if alive.len() == 1 {
            self.state.winner = Some(alive[0]);
        }
    }

    fn player_stats(&self) -> Vec<PlayerStats> {
        (0..self.state.num_players)
            .map(|p| PlayerStats {
                player: p,
                land: self.state.land_count(p),
                armies: self.state.army_count(p),
                alive: self.state.alive[p as usize],
            })
            .collect()
    }
}
