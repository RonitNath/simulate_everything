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
    /// Deprecated — city garrison is now computed from distance and map size.
    /// Kept for backwards compatibility but unused by `place_cities`.
    pub city_army_range: (i32, i32),
    /// Minimum BFS distance between any two generals.
    pub min_general_distance: usize,
    /// Minimum distance (Manhattan) from edge for general placement.
    pub general_margin: usize,
    /// Minimum Manhattan distance from generals to nearest city.
    pub city_general_buffer: usize,
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            width: 16,
            height: 16,
            num_players: 2,
            mountain_density: 0.2,
            num_cities: 8,
            city_army_range: (20, 50),
            min_general_distance: 9,
            general_margin: 3,
            city_general_buffer: 5,
        }
    }
}

impl MapConfig {
    pub fn for_players(num_players: u8) -> Self {
        // Scale board size with player count.
        // 5× base area, shaped ~16:9 to fill widescreen displays.
        let base_side = (12 + (num_players as usize) * 3) * 2;
        let base_area = base_side * base_side;
        let area = base_area * 5;
        // height = sqrt(area * 9/16), width = area / height
        let height = ((area as f64 * 9.0 / 16.0).sqrt()).round() as usize;
        let width = area / height;
        Self::for_size(width, height, num_players)
    }

    /// Build a config for a specific board size. Derives city count, distances,
    /// etc. from the dimensions so overriding width/height doesn't leave stale
    /// values.
    pub fn for_size(width: usize, height: usize, num_players: u8) -> Self {
        let area = width * height;
        // ~3% of non-mountain cells should be cities.
        let num_cities = (area as f64 * 0.03).round() as usize;
        let min_side = width.min(height);
        // General distance: 40-60% of the smaller dimension for variety.
        let min_general_distance = (min_side * 2 / 5).max(6);
        // Margin from edge: ~15% of smaller dimension, at least 3.
        let general_margin = (min_side / 7).max(3);
        // Cities stay away from generals — mid-game mechanic.
        let city_general_buffer = (min_side / 5).max(4);
        Self {
            width,
            height,
            num_players,
            mountain_density: 0.2,
            num_cities,
            city_army_range: (20, 50),
            min_general_distance,
            general_margin,
            city_general_buffer,
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

    // 2. Place mountains in clusters (ridges and formations).
    let mountain_count = ((n as f32) * config.mountain_density) as usize;
    place_mountain_clusters(&mut grid, w, h, mountain_count, &generals, rng);

    // 3. Verify all generals are connected via BFS on non-mountain cells.
    if !all_connected(&grid, w, h, &generals) {
        return None;
    }

    // 4. Place cities — biased away from generals (mid-game mechanic).
    place_cities(&mut grid, w, h, config, &generals, rng);

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

/// Place mountains in clusters. Picks seed points, then grows ridges outward
/// via random walks. Remaining budget filled with scattered singles.
fn place_mountain_clusters(
    grid: &mut [Cell],
    w: usize,
    h: usize,
    target: usize,
    generals: &[(usize, usize)],
    rng: &mut impl Rng,
) {
    let avg_cluster = 5;
    let num_seeds = (target / avg_cluster).max(1);
    let mut placed = 0;

    let too_close_to_general = |row: usize, col: usize| -> bool {
        generals.iter().any(|&(gr, gc)| {
            let dr = (row as i32 - gr as i32).unsigned_abs() as usize;
            let dc = (col as i32 - gc as i32).unsigned_abs() as usize;
            dr <= 1 && dc <= 1
        })
    };

    let try_place = |grid: &mut [Cell], row: usize, col: usize, placed: &mut usize, target: usize| -> bool {
        if *placed >= target { return false; }
        if row >= h || col >= w { return false; }
        let idx = row * w + col;
        if grid[idx].tile != Tile::Empty { return false; }
        grid[idx] = Cell::mountain();
        *placed += 1;
        true
    };

    // Phase 1: Grow clusters from random seeds (~70% of budget).
    let cluster_budget = target * 7 / 10;
    for _ in 0..num_seeds {
        if placed >= cluster_budget { break; }

        // Pick a seed point not near generals.
        let mut seed = None;
        for _ in 0..50 {
            let row = rng.gen_range(0..h);
            let col = rng.gen_range(0..w);
            if !too_close_to_general(row, col) && grid[row * w + col].tile == Tile::Empty {
                seed = Some((row, col));
                break;
            }
        }
        let Some((mut r, mut c)) = seed else { continue };

        // Random walk from seed, placing mountains along the way.
        let walk_len = rng.gen_range(3..=avg_cluster + 3);
        for _ in 0..walk_len {
            if too_close_to_general(r, c) { break; }
            try_place(grid, r, c, &mut placed, cluster_budget);

            // Pick a random cardinal direction, biased to continue straight.
            let dirs: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
            let (dr, dc) = dirs[rng.gen_range(0..4)];
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr >= 0 && nc >= 0 && (nr as usize) < h && (nc as usize) < w {
                r = nr as usize;
                c = nc as usize;
            }

            // Occasional branch: place a perpendicular neighbor.
            if rng.gen_ratio(1, 3) {
                let perp = if dr == 0 {
                    [(r.wrapping_sub(1), c), (r + 1, c)]
                } else {
                    [(r, c.wrapping_sub(1)), (r, c + 1)]
                };
                let &(br, bc) = &perp[rng.gen_range(0..2)];
                if br < h && bc < w && !too_close_to_general(br, bc) {
                    try_place(grid, br, bc, &mut placed, cluster_budget);
                }
            }
        }
    }

    // Phase 2: Fill remaining budget with scattered singles.
    let mut attempts = 0;
    let n = w * h;
    while placed < target && attempts < n * 4 {
        let row = rng.gen_range(0..h);
        let col = rng.gen_range(0..w);
        if !too_close_to_general(row, col) {
            try_place(grid, row, col, &mut placed, target);
        }
        attempts += 1;
    }
}

/// Place cities with a buffer zone around generals. Cities are a mid-game
/// mechanic so they shouldn't appear in a player's starting area.
fn place_cities(
    grid: &mut [Cell],
    w: usize,
    h: usize,
    config: &MapConfig,
    generals: &[(usize, usize)],
    rng: &mut impl Rng,
) {
    let n = w * h;
    let min_side = w.min(h);
    // Max possible Manhattan distance on this board (used to normalize).
    let max_dist = (w + h) as f64;
    let mut cities_placed = 0;
    let mut attempts = 0;
    while cities_placed < config.num_cities && attempts < n * 4 {
        let row = rng.gen_range(0..h);
        let col = rng.gen_range(0..w);
        let idx = row * w + col;
        if grid[idx].tile == Tile::Empty {
            let nearest_general_dist = generals.iter().map(|&(gr, gc)| {
                let dr = (row as i32 - gr as i32).unsigned_abs() as usize;
                let dc = (col as i32 - gc as i32).unsigned_abs() as usize;
                dr + dc
            }).min().unwrap_or(0);

            if nearest_general_dist < config.city_general_buffer {
                attempts += 1;
                continue;
            }

            // Scale garrison by distance from nearest general and map size.
            // Closer cities (just past the buffer) are cheaper; distant/central
            // cities are more expensive — they're the mid-game flashpoints.
            let dist_frac = nearest_general_dist as f64 / max_dist;
            // Base garrison scales with map size: bigger maps need beefier cities
            // so they remain a mid-game investment, not a trivial early capture.
            let base = (min_side as f64 * 0.8).round() as i32; // ~18 on 23x23, ~40 on 50x50
            let range = (min_side as f64 * 0.6).round() as i32;
            // Distance multiplier: 0.5 for close cities, up to 1.5 for far ones.
            let dist_mult = 0.5 + dist_frac * 2.0;
            let lo = ((base as f64 * dist_mult) as i32).max(10);
            let hi = (((base + range) as f64 * dist_mult) as i32).max(lo + 5);
            let army = rng.gen_range(lo..=hi);
            grid[idx] = Cell::city(army);
            cities_placed += 1;
        }
        attempts += 1;
    }
}

fn place_generals(config: &MapConfig, rng: &mut impl Rng) -> Option<Vec<(usize, usize)>> {
    let w = config.width;
    let h = config.height;
    let margin = config.general_margin;

    // Ensure margin doesn't consume the entire board.
    if margin * 2 >= w || margin * 2 >= h {
        return None;
    }

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
