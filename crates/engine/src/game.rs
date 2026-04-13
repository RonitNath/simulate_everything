use crate::action::Action;
use crate::agent::Observation;
use crate::event::{Event, PlayerAction, PlayerStats};
use crate::state::{GameState, Tile};

/// Runs the game loop. Manages turn execution, army growth, and win conditions.
pub struct Game {
    pub state: GameState,
    pub max_turns: u32,
    pub events: Vec<Event>,
}

impl Game {
    pub fn new(state: GameState, max_turns: u32) -> Self {
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
        }
    }

    pub fn is_over(&self) -> bool {
        self.state.winner.is_some() || self.state.turn >= self.max_turns
    }

    /// Get observations for all alive players.
    pub fn observations(&self) -> Vec<Observation> {
        (0..self.state.num_players)
            .map(|p| Observation::from_state(&self.state, p))
            .collect()
    }

    /// Execute one turn: collect actions, resolve moves, grow armies, check wins.
    pub fn step(&mut self, actions: &[(u8, Action)]) {
        let mut player_actions = Vec::new();

        // Execute each player's action.
        for &(player, action) in actions {
            if !self.state.alive[player as usize] {
                player_actions.push(PlayerAction {
                    player,
                    action,
                    valid: false,
                });
                continue;
            }

            let valid = self.execute_action(player, action);
            player_actions.push(PlayerAction {
                player,
                action,
                valid,
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
            return true; // Pass is always valid.
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

        // Calculate armies to send.
        let src_armies = self.state.cell(row, col).armies;
        let send = if split {
            src_armies / 2
        } else {
            src_armies - 1
        };

        if send <= 0 {
            return false;
        }

        // Deduct from source.
        self.state.cell_mut(row, col).armies -= send;

        // Resolve at destination.
        let dst = self.state.cell(nr, nc);
        let dst_owner = dst.owner;
        let dst_armies = dst.armies;

        match dst_owner {
            Some(owner) if owner == player => {
                // Reinforce.
                self.state.cell_mut(nr, nc).armies += send;
            }
            Some(defender) => {
                // Combat.
                if send > dst_armies {
                    // Attacker wins.
                    let remaining = send - dst_armies;
                    let dst_cell = self.state.cell_mut(nr, nc);
                    dst_cell.armies = remaining;
                    dst_cell.owner = Some(player);

                    // Check if we captured a general.
                    if dst_cell.tile == Tile::General {
                        self.eliminate(defender, player);
                    }
                } else {
                    // Defender holds.
                    self.state.cell_mut(nr, nc).armies = dst_armies - send;
                }
            }
            None => {
                // Capture neutral.
                if dst.tile == Tile::City {
                    // Must overcome city garrison.
                    if send > dst_armies {
                        let dst_cell = self.state.cell_mut(nr, nc);
                        dst_cell.armies = send - dst_armies;
                        dst_cell.owner = Some(player);
                    } else {
                        self.state.cell_mut(nr, nc).armies = dst_armies - send;
                    }
                } else {
                    // Empty cell, just take it.
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

        // Transfer all loser's cells to winner.
        for cell in &mut self.state.grid {
            if cell.owner == Some(loser) {
                cell.owner = Some(winner);
            }
        }

        // Turn the captured general into a city (it's now owned by winner).
        let (gr, gc) = self.state.general_positions[loser as usize];
        self.state.cell_mut(gr, gc).tile = Tile::City;

        self.events.push(Event::Elimination {
            turn: self.state.turn,
            eliminated: loser,
            by: winner,
        });
    }

    fn grow_armies(&mut self) {
        let turn = self.state.turn;

        // Every 2 turns: owned generals and cities get +1.
        if turn % 2 == 0 {
            for cell in &mut self.state.grid {
                if cell.owner.is_some()
                    && (cell.tile == Tile::General || cell.tile == Tile::City)
                {
                    cell.armies += 1;
                }
            }
        }

        // Every 50 turns: all owned cells get +1.
        if turn > 0 && turn % 50 == 0 {
            for cell in &mut self.state.grid {
                if cell.owner.is_some() {
                    cell.armies += 1;
                }
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
