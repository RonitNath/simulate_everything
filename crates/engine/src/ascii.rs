use crate::event::PlayerStats;
use crate::replay::Frame;
use crate::state::{Cell, GameState, Tile};
use std::fmt;

/// Player labels: a, b, c, ... for owned cells; A, B, C, ... for generals.
fn player_char(player: u8, general: bool) -> char {
    let base = b'a' + player;
    if general {
        (base - 32) as char
    } else {
        base as char
    }
}

/// Render a single cell as a fixed-width token (width = `col_w`).
///
/// Encoding:
///   `....` — empty unowned
///   `####` — mountain
///   `c 42` — neutral city (42 armies)
///   `a  5` — player a, 5 armies
///   `A 38` — player a's general, 38 armies
///   `a~12` — player a's city, 12 armies
fn render_cell(cell: &Cell, buf: &mut String, col_w: usize) {
    match (cell.tile, cell.owner) {
        (Tile::Mountain, _) => {
            for _ in 0..col_w {
                buf.push('#');
            }
        }
        (Tile::Empty | Tile::City | Tile::General, None) => {
            if cell.tile == Tile::City {
                // Neutral city: "c" + armies right-aligned
                let army_str = format!("{}", cell.armies);
                buf.push('c');
                for _ in 0..(col_w - 1 - army_str.len()) {
                    buf.push(' ');
                }
                buf.push_str(&army_str);
            } else {
                // Empty unowned
                for _ in 0..col_w {
                    buf.push('.');
                }
            }
        }
        (tile, Some(p)) => {
            let ch = player_char(p, tile == Tile::General);
            let marker = if tile == Tile::City { '~' } else { ' ' };
            let army_str = format!("{}", cell.armies);
            buf.push(ch);
            if col_w > 1 + army_str.len() {
                buf.push(marker);
                for _ in 0..(col_w - 2 - army_str.len()) {
                    buf.push(' ');
                }
            }
            buf.push_str(&army_str);
        }
    }
}

/// Determine column width needed for the widest cell in the grid.
fn col_width(grid: &[Cell]) -> usize {
    let max_army = grid.iter().map(|c| c.armies.abs()).max().unwrap_or(0);
    let digits = if max_army == 0 {
        1
    } else {
        (max_army as f64).log10().floor() as usize + 1
    };
    // prefix char + marker + digits, minimum 4
    (2 + digits).max(4)
}

/// Format the grid into an ASCII string.
///
/// ```text
/// Turn 42 | a: 15 land 127 army | b: 12 land 98 army
///      0    1    2    3    4
///  0  .... .... #### a  5 a  3
///  1  .... b  2 .... a  8 ####
///  2  c 40 .... .... .... B 15
/// ```
pub fn screenshot(
    width: usize,
    height: usize,
    grid: &[Cell],
    turn: u32,
    stats: Option<&[PlayerStats]>,
) -> String {
    let cw = col_width(grid);
    let row_label_w = format!("{}", height.saturating_sub(1)).len() + 1;
    let mut out = String::with_capacity(height * width * (cw + 1) + 256);

    // Header
    out.push_str(&format!("Turn {}", turn));
    if let Some(stats) = stats {
        for s in stats {
            let ch = player_char(s.player, false);
            let alive = if s.alive { "" } else { " DEAD" };
            out.push_str(&format!(
                " | {}: {} land {} army{}",
                ch, s.land, s.armies, alive
            ));
        }
    }
    out.push('\n');

    // Column headers
    for _ in 0..row_label_w {
        out.push(' ');
    }
    for c in 0..width {
        let label = format!("{}", c);
        let pad = cw - label.len();
        let left = pad / 2;
        let right = pad - left;
        for _ in 0..left {
            out.push(' ');
        }
        out.push_str(&label);
        for _ in 0..right {
            out.push(' ');
        }
        out.push(' ');
    }
    out.push('\n');

    // Grid rows
    for r in 0..height {
        let label = format!("{}", r);
        for _ in 0..(row_label_w - label.len()) {
            out.push(' ');
        }
        out.push_str(&label);
        for c in 0..width {
            if c > 0 {
                out.push(' ');
            }
            render_cell(&grid[r * width + c], &mut out, cw);
        }
        out.push('\n');
    }

    out
}

/// Display impl for GameState — prints the full ASCII board.
impl fmt::Display for GameState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stats: Vec<PlayerStats> = (0..self.num_players)
            .map(|p| PlayerStats {
                player: p,
                land: self.land_count(p),
                armies: self.army_count(p),
                alive: self.alive[p as usize],
            })
            .collect();
        write!(
            f,
            "{}",
            screenshot(self.width, self.height, &self.grid, self.turn, Some(&stats),)
        )
    }
}

/// A Frame doesn't carry width/height, so this wrapper enables Display.
pub struct FrameView<'a> {
    pub frame: &'a Frame,
    pub width: usize,
    pub height: usize,
}

impl fmt::Display for FrameView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            screenshot(
                self.width,
                self.height,
                &self.frame.grid,
                self.frame.turn,
                Some(&self.frame.stats),
            )
        )
    }
}

impl Frame {
    pub fn ascii(&self, width: usize, height: usize) -> FrameView<'_> {
        FrameView {
            frame: self,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Cell, Tile};

    #[test]
    fn basic_screenshot() {
        let grid = vec![
            Cell {
                tile: Tile::General,
                owner: Some(0),
                armies: 1,
            },
            Cell::empty(),
            Cell::mountain(),
            Cell::empty(),
            Cell::empty(),
            Cell {
                tile: Tile::City,
                owner: None,
                armies: 40,
            },
            Cell::mountain(),
            Cell::empty(),
            Cell {
                tile: Tile::General,
                owner: Some(1),
                armies: 12,
            },
        ];
        let out = screenshot(3, 3, &grid, 0, None);
        assert!(out.contains("Turn 0"));
        assert!(out.contains("A  1")); // player a's general
        assert!(out.contains("####")); // mountain
        assert!(out.contains("c 40")); // neutral city
        assert!(out.contains("B 12")); // player b's general
    }

    #[test]
    fn owned_city_marker() {
        let grid = vec![Cell {
            tile: Tile::City,
            owner: Some(0),
            armies: 7,
        }];
        let out = screenshot(1, 1, &grid, 5, None);
        assert!(
            out.contains("a~"),
            "owned city should have ~ marker: {}",
            out
        );
    }

    #[test]
    fn dead_player_stats() {
        let grid = vec![Cell::empty()];
        let stats = vec![
            PlayerStats {
                player: 0,
                land: 10,
                armies: 50,
                alive: true,
            },
            PlayerStats {
                player: 1,
                land: 0,
                armies: 0,
                alive: false,
            },
        ];
        let out = screenshot(1, 1, &grid, 99, Some(&stats));
        assert!(out.contains("DEAD"));
    }

    #[test]
    fn game_state_display() {
        let state = GameState {
            width: 2,
            height: 2,
            grid: vec![
                Cell::general(0),
                Cell::empty(),
                Cell::empty(),
                Cell::general(1),
            ],
            num_players: 2,
            general_positions: vec![(0, 0), (1, 1)],
            alive: vec![true, true],
            turn: 0,
            winner: None,
        };
        let out = format!("{}", state);
        assert!(out.contains("Turn 0"));
        assert!(out.contains("a: 1 land 1 army"));
    }
}
