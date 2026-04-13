use crate::state::{Cell, GameState, Tile};
use rand::Rng;
use std::collections::VecDeque;

/// Map generation parameters.
pub struct MapConfig {
    pub width: usize,
    pub height: usize,
    pub num_players: u8,
    /// Fraction of cells that are mountains (0.0 - 1.0).
    pub mountain_density: f32,
    /// Number of neutral cities.
    pub num_cities: usize,
    /// Army value range for neutral cities.
    pub city_army_range: (i32, i32),
    /// Minimum BFS distance between any two generals.
    pub min_general_distance: usize,
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            width: 16,
            height: 16,
            num_players: 2,
            mountain_density: 0.2,
            num_cities: 8,
            city_army_range: (40, 50),
            min_general_distance: 10,
        }
    }
}

impl MapConfig {
    pub fn for_players(num_players: u8) -> Self {
        // Scale board size with player count.
        let side = 12 + (num_players as usize) * 3;
        let cities = (num_players as usize) * 3 + 2;
        Self {
            width: side,
            height: side,
            num_players,
            num_cities: cities,
            min_general_distance: side / 2,
            ..Default::default()
        }
    }
}

/// Generate a game state with the given config.
/// Retries internally until it produces a connected, fair map.
pub fn generate(config: &MapConfig, rng: &mut impl Rng) -> GameState {
    for _ in 0..1000 {
        if let Some(state) = try_generate(config, rng) {
            return state;
        }
    }
    panic!("Failed to generate a valid map after 1000 attempts");
}

fn try_generate(config: &MapConfig, rng: &mut impl Rng) -> Option<GameState> {
    let w = config.width;
    let h = config.height;
    let n = w * h;
    let mut grid = vec![Cell::empty(); n];

    // 1. Place generals with minimum distance constraint.
    let generals = place_generals(config, rng)?;

    for (player, &(row, col)) in generals.iter().enumerate() {
        grid[row * w + col] = Cell::general(player as u8);
    }

    // 2. Place mountains.
    let mountain_count = ((n as f32) * config.mountain_density) as usize;
    let mut placed_mountains = 0;
    let mut attempts = 0;
    while placed_mountains < mountain_count && attempts < n * 4 {
        let row = rng.gen_range(0..h);
        let col = rng.gen_range(0..w);
        let idx = row * w + col;
        if grid[idx].tile == Tile::Empty {
            // Don't place mountains adjacent to generals.
            let too_close = generals.iter().any(|&(gr, gc)| {
                let dr = (row as i32 - gr as i32).unsigned_abs() as usize;
                let dc = (col as i32 - gc as i32).unsigned_abs() as usize;
                dr <= 1 && dc <= 1
            });
            if !too_close {
                grid[idx] = Cell::mountain();
                placed_mountains += 1;
            }
        }
        attempts += 1;
    }

    // 3. Verify all generals are connected via BFS on non-mountain cells.
    if !all_connected(&grid, w, h, &generals) {
        return None;
    }

    // 4. Place cities.
    let mut cities_placed = 0;
    attempts = 0;
    while cities_placed < config.num_cities && attempts < n * 4 {
        let row = rng.gen_range(0..h);
        let col = rng.gen_range(0..w);
        let idx = row * w + col;
        if grid[idx].tile == Tile::Empty {
            // Don't place cities too close to generals (min 3 tiles).
            let too_close = generals.iter().any(|&(gr, gc)| {
                let dr = (row as i32 - gr as i32).unsigned_abs() as usize;
                let dc = (col as i32 - gc as i32).unsigned_abs() as usize;
                dr + dc < 3
            });
            if !too_close {
                let army = rng.gen_range(config.city_army_range.0..=config.city_army_range.1);
                grid[idx] = Cell::city(army);
                cities_placed += 1;
            }
        }
        attempts += 1;
    }

    Some(GameState {
        width: w,
        height: h,
        grid,
        num_players: config.num_players,
        general_positions: generals,
        alive: vec![true; config.num_players as usize],
        turn: 0,
        winner: None,
    })
}

fn place_generals(config: &MapConfig, rng: &mut impl Rng) -> Option<Vec<(usize, usize)>> {
    let w = config.width;
    let h = config.height;
    let margin = 2usize;

    for _ in 0..500 {
        let mut positions = Vec::new();
        let mut ok = true;

        for _ in 0..config.num_players {
            let mut found = false;
            for _ in 0..200 {
                let row = rng.gen_range(margin..h - margin);
                let col = rng.gen_range(margin..w - margin);

                let far_enough = positions.iter().all(|&(r, c): &(usize, usize)| {
                    let dr = (row as i32 - r as i32).unsigned_abs() as usize;
                    let dc = (col as i32 - c as i32).unsigned_abs() as usize;
                    dr + dc >= config.min_general_distance
                });

                if far_enough {
                    positions.push((row, col));
                    found = true;
                    break;
                }
            }
            if !found {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(positions);
        }
    }
    None
}

fn all_connected(grid: &[Cell], w: usize, h: usize, generals: &[(usize, usize)]) -> bool {
    if generals.is_empty() {
        return true;
    }

    let n = w * h;
    let mut visited = vec![false; n];
    let mut queue = VecDeque::new();

    let start = generals[0];
    queue.push_back(start);
    visited[start.0 * w + start.1] = true;

    while let Some((r, c)) = queue.pop_front() {
        for (dr, dc) in [(-1i32, 0), (1, 0), (0, -1), (0, 1)] {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                let ni = nr as usize * w + nc as usize;
                if !visited[ni] && grid[ni].tile != Tile::Mountain {
                    visited[ni] = true;
                    queue.push_back((nr as usize, nc as usize));
                }
            }
        }
    }

    generals
        .iter()
        .all(|&(r, c)| visited[r * w + c])
}
