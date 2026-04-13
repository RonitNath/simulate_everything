use noise::{NoiseFn, Perlin};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::hex::{axial_to_offset, distance, offset_to_axial, within_radius};
use super::state::{
    Biome, Cell, GameState, Player, Population, Region, RegionArchetype, Role, Unit,
};
use super::{INITIAL_STRENGTH, INITIAL_UNITS};

pub struct MapConfig {
    pub width: usize,
    pub height: usize,
    pub num_players: u8,
    pub seed: u64,
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            width: 60,
            height: 60,
            num_players: 2,
            seed: 42,
        }
    }
}

pub fn generate(config: &MapConfig) -> GameState {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let height_noise = Perlin::new(config.seed as u32);
    let moisture_noise = Perlin::new((config.seed as u32).wrapping_add(1));
    let river_noise = Perlin::new((config.seed as u32).wrapping_add(2));

    let total = config.width * config.height;
    let mut grid = Vec::with_capacity(total);
    let region_cols = ((config.width / 15).max(1)).min(6);
    let region_rows = ((config.height / 15).max(1)).min(6);
    let region_count = region_cols * region_rows;
    let mut region_hexes: Vec<Vec<super::hex::Axial>> = vec![Vec::new(); region_count];
    let mut region_fertility = vec![0.0f32; region_count];
    let mut region_minerals = vec![0.0f32; region_count];
    let mut region_heights = vec![0.0f32; region_count];

    for row in 0..config.height {
        for col in 0..config.width {
            let wx = col as f64 + 0.5 * (row & 1) as f64;
            let wy = row as f64 * 0.866;

            let h1 = height_noise.get([wx * 0.02, wy * 0.02]);
            let h2 = height_noise.get([wx * 0.05, wy * 0.05]) * 0.5;
            let h3 = height_noise.get([wx * 0.12, wy * 0.12]) * 0.25;
            let height = (((h1 + h2 + h3) / 1.75) * 0.5 + 0.5) as f32;

            let m1 = moisture_noise.get([wx * 0.025, wy * 0.025]);
            let m2 = moisture_noise.get([wx * 0.08, wy * 0.08]) * 0.4;
            let moisture = ((((m1 + m2) / 1.4) * 0.5) + 0.5) as f32;

            let river = river_noise.get([wx * 0.035, wy * 0.035]) as f32;
            let is_river = river.abs() < 0.035 && height > 0.25 && height < 0.72;
            let water_access = (moisture * 0.7 + if is_river { 0.3 } else { 0.0 }).clamp(0.0, 1.0);

            let biome = classify_biome(height, moisture, is_river);
            let terrain_value = fertility_for_biome(biome, moisture, water_access);
            let material_value = mineral_value(height, biome);

            let region_id = compute_region_id(
                row,
                col,
                config.width,
                config.height,
                region_cols,
                region_rows,
            );
            let axial = offset_to_axial(row as i32, col as i32);
            region_hexes[region_id as usize].push(axial);
            region_fertility[region_id as usize] += terrain_value;
            region_minerals[region_id as usize] += material_value;
            region_heights[region_id as usize] += height;

            grid.push(Cell {
                terrain_value,
                material_value,
                food_stockpile: 0.0,
                material_stockpile: 0.0,
                has_depot: false,
                road_level: 0,
                height,
                moisture,
                biome,
                is_river,
                water_access,
                region_id,
                stockpile_owner: None,
            });
        }
    }

    let regions = build_regions(
        region_hexes,
        region_fertility,
        region_minerals,
        region_heights,
    );

    let strategic_values = compute_strategic_values(&grid, config.width, config.height);
    let margin = (config.width.max(config.height) / 8).max(5);
    let general_positions = place_generals(config, &grid, &strategic_values, margin, &mut rng);

    let mut units: Vec<Unit> = Vec::new();
    let mut players: Vec<Player> = Vec::new();
    let mut population: Vec<Population> = Vec::new();
    let mut next_id: u32 = 0;
    let mut next_pop_id: u32 = 0;

    for (player_idx, &gen_pos) in general_positions.iter().enumerate() {
        let owner = player_idx as u8;

        // Spawn the general unit
        let general_id = next_id;
        next_id += 1;
        units.push(Unit {
            id: general_id,
            owner,
            pos: gen_pos,
            strength: INITIAL_STRENGTH,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: true,
        });

        // Spawn INITIAL_UNITS nearby units
        let mut placed = 0;
        let mut candidates: Vec<_> = within_radius(gen_pos, 3)
            .into_iter()
            .filter(|&ax| ax != gen_pos)
            .collect();
        // Shuffle candidates for determinism
        for i in (1..candidates.len()).rev() {
            let j = rng.gen_range(0..=i);
            candidates.swap(i, j);
        }
        for candidate in candidates {
            if placed >= INITIAL_UNITS {
                break;
            }
            if !is_in_bounds(candidate, config.width, config.height) {
                continue;
            }
            units.push(Unit {
                id: next_id,
                owner,
                pos: candidate,
                strength: INITIAL_STRENGTH,
                move_cooldown: 0,
                engagements: Vec::new(),
                destination: None,
                is_general: false,
            });
            next_id += 1;
            placed += 1;
        }

        players.push(Player {
            id: owner,
            food: 0.0,
            material: 0.0,
            general_id,
            alive: true,
        });

        let mut push_pop = |count: u16, role: Role| {
            population.push(Population {
                id: next_pop_id,
                hex: gen_pos,
                owner,
                count,
                role,
                training: if role == Role::Soldier { 1.0 } else { 0.0 },
            });
            next_pop_id += 1;
        };
        push_pop(20, Role::Idle);
        push_pop(5, Role::Farmer);
        push_pop(3, Role::Worker);

        if let Some(cell) = grid_at_mut(&mut grid, config.width, gen_pos) {
            cell.stockpile_owner = Some(owner);
            cell.food_stockpile = 80.0;
            cell.material_stockpile = 50.0;
        }
    }

    let mut state = GameState {
        width: config.width,
        height: config.height,
        grid,
        units,
        players,
        population,
        convoys: Vec::new(),
        regions,
        tick: 0,
        next_unit_id: next_id,
        next_pop_id,
        next_convoy_id: 0,
    };
    recompute_player_totals(&mut state);

    state
}

fn is_in_bounds(ax: super::hex::Axial, width: usize, height: usize) -> bool {
    let (row, col) = axial_to_offset(ax);
    row >= 0 && col >= 0 && (row as usize) < height && (col as usize) < width
}

fn grid_at_mut(grid: &mut [Cell], width: usize, ax: super::hex::Axial) -> Option<&mut Cell> {
    let (row, col) = axial_to_offset(ax);
    if row < 0 || col < 0 {
        return None;
    }
    let idx = row as usize * width + col as usize;
    grid.get_mut(idx)
}

fn classify_biome(height: f32, moisture: f32, is_river: bool) -> Biome {
    if height > 0.82 {
        Biome::Mountain
    } else if height < 0.18 {
        Biome::Desert
    } else if moisture < 0.2 {
        Biome::Desert
    } else if moisture < 0.35 {
        Biome::Steppe
    } else if moisture < 0.6 {
        if height > 0.65 {
            Biome::Tundra
        } else {
            Biome::Grassland
        }
    } else if moisture < 0.8 {
        Biome::Forest
    } else if is_river || height < 0.45 {
        Biome::Jungle
    } else {
        Biome::Forest
    }
}

fn fertility_for_biome(biome: Biome, moisture: f32, water_access: f32) -> f32 {
    let base = match biome {
        Biome::Desert => 0.2,
        Biome::Steppe => 0.9,
        Biome::Grassland => 1.5,
        Biome::Forest => 1.3,
        Biome::Jungle => 1.7,
        Biome::Tundra => 0.6,
        Biome::Mountain => 0.4,
    };
    (base + moisture * 0.8 + water_access * 0.6).clamp(0.0, 3.0)
}

fn mineral_value(height: f32, biome: Biome) -> f32 {
    let rugged_bonus = (height * 1.5).clamp(0.0, 1.5);
    let biome_bonus = match biome {
        Biome::Mountain => 0.8,
        Biome::Forest => 0.4,
        Biome::Steppe => 0.2,
        _ => 0.1,
    };
    (rugged_bonus + biome_bonus).clamp(0.0, 2.0)
}

fn compute_region_id(
    row: usize,
    col: usize,
    width: usize,
    height: usize,
    region_cols: usize,
    region_rows: usize,
) -> u16 {
    let region_w = ((width as f32 / region_cols as f32).ceil() as usize).max(1);
    let region_h = ((height as f32 / region_rows as f32).ceil() as usize).max(1);
    let rx = (col / region_w).min(region_cols - 1);
    let ry = (row / region_h).min(region_rows - 1);
    (ry * region_cols + rx) as u16
}

fn build_regions(
    region_hexes: Vec<Vec<super::hex::Axial>>,
    fertility: Vec<f32>,
    minerals: Vec<f32>,
    heights: Vec<f32>,
) -> Vec<Region> {
    region_hexes
        .into_iter()
        .enumerate()
        .map(|(idx, hexes)| {
            let count = hexes.len().max(1) as f32;
            let avg_fertility = fertility[idx] / count;
            let avg_minerals = minerals[idx] / count;
            let avg_height = heights[idx] / count;
            let archetype = if avg_height > 0.75 {
                RegionArchetype::MountainRange
            } else if avg_minerals > 1.25 {
                RegionArchetype::Highland
            } else if avg_fertility > 2.0 {
                RegionArchetype::RiverValley
            } else if avg_fertility < 0.8 {
                RegionArchetype::Desert
            } else {
                RegionArchetype::Steppe
            };
            Region {
                id: idx as u16,
                name: format!("{:?} {}", archetype, idx + 1),
                archetype,
                hexes,
                avg_fertility,
                avg_minerals,
                defensibility: (avg_height + avg_minerals * 0.5).clamp(0.0, 2.0),
            }
        })
        .collect()
}

fn recompute_player_totals(state: &mut GameState) {
    for player in &mut state.players {
        player.food = 0.0;
        player.material = 0.0;
    }
    for cell in &state.grid {
        if let Some(owner) = cell.stockpile_owner {
            if let Some(player) = state.players.iter_mut().find(|p| p.id == owner) {
                player.food += cell.food_stockpile;
                player.material += cell.material_stockpile;
            }
        }
    }
}

fn compute_strategic_values(grid: &[Cell], width: usize, height: usize) -> Vec<f32> {
    let radius = 10i32;
    let mut sv = Vec::with_capacity(grid.len());

    for row in 0..height {
        for col in 0..width {
            let center = offset_to_axial(row as i32, col as i32);
            let mut sum = 0.0f32;
            let nearby = within_radius(center, radius);
            for neighbor in nearby {
                let (nr, nc) = axial_to_offset(neighbor);
                if nr >= 0 && nc >= 0 && (nr as usize) < height && (nc as usize) < width {
                    sum += grid[(nr as usize) * width + (nc as usize)].terrain_value;
                }
            }
            sv.push(sum);
        }
    }
    sv
}

fn place_generals(
    config: &MapConfig,
    grid: &[Cell],
    strategic_values: &[f32],
    margin: usize,
    rng: &mut StdRng,
) -> Vec<super::hex::Axial> {
    // Collect candidates: terrain > 1.0, far enough from edge
    let candidates: Vec<_> = (0..config.height)
        .flat_map(|row| (0..config.width).map(move |col| (row, col)))
        .filter(|&(row, col)| {
            let idx = row * config.width + col;
            grid[idx].terrain_value > 1.0
                && row >= margin
                && row + margin < config.height
                && col >= margin
                && col + margin < config.width
        })
        .collect();

    if config.num_players == 2 {
        place_generals_2p(config, grid, strategic_values, &candidates, rng)
    } else {
        place_generals_np(config, grid, strategic_values, &candidates, rng)
    }
}

fn place_generals_2p(
    config: &MapConfig,
    _grid: &[Cell],
    strategic_values: &[f32],
    candidates: &[(usize, usize)],
    rng: &mut StdRng,
) -> Vec<super::hex::Axial> {
    // Score pairs by: distance - abs(sv_diff) * weight
    // For large maps, sample a subset of candidate pairs
    let max_candidates = candidates.len().min(200);
    let sampled: Vec<_> = if candidates.len() > max_candidates {
        let mut indices: Vec<usize> = (0..candidates.len()).collect();
        for i in (1..indices.len()).rev() {
            let j = rng.gen_range(0..=i);
            indices.swap(i, j);
        }
        indices[..max_candidates]
            .iter()
            .map(|&i| candidates[i])
            .collect()
    } else {
        candidates.to_vec()
    };

    let sv_weight = 0.1f32;
    let mut best_score = f32::NEG_INFINITY;
    let mut best_pair = (
        offset_to_axial(0, 0),
        offset_to_axial(config.height as i32 - 1, config.width as i32 - 1),
    );

    for i in 0..sampled.len() {
        for j in (i + 1)..sampled.len() {
            let (r1, c1) = sampled[i];
            let (r2, c2) = sampled[j];
            let ax1 = offset_to_axial(r1 as i32, c1 as i32);
            let ax2 = offset_to_axial(r2 as i32, c2 as i32);
            let dist = distance(ax1, ax2) as f32;
            let sv1 = strategic_values[r1 * config.width + c1];
            let sv2 = strategic_values[r2 * config.width + c2];
            let sv_diff = (sv1 - sv2).abs();
            let score = dist - sv_diff * sv_weight;
            if score > best_score {
                best_score = score;
                best_pair = (ax1, ax2);
            }
        }
    }

    vec![best_pair.0, best_pair.1]
}

fn place_generals_np(
    config: &MapConfig,
    _grid: &[Cell],
    strategic_values: &[f32],
    candidates: &[(usize, usize)],
    rng: &mut StdRng,
) -> Vec<super::hex::Axial> {
    let max_candidates = candidates.len().min(300);
    let sampled: Vec<_> = if candidates.len() > max_candidates {
        let mut indices: Vec<usize> = (0..candidates.len()).collect();
        for i in (1..indices.len()).rev() {
            let j = rng.gen_range(0..=i);
            indices.swap(i, j);
        }
        indices[..max_candidates]
            .iter()
            .map(|&i| candidates[i])
            .collect()
    } else {
        candidates.to_vec()
    };

    let mut placed: Vec<super::hex::Axial> = Vec::new();

    // First general: pick one with median strategic value
    if sampled.is_empty() {
        // Fallback: just use corners
        for i in 0..config.num_players as usize {
            placed.push(offset_to_axial(i as i32, i as i32));
        }
        return placed;
    }

    let first_idx = rng.gen_range(0..sampled.len());
    let (r0, c0) = sampled[first_idx];
    placed.push(offset_to_axial(r0 as i32, c0 as i32));

    let avg_sv = strategic_values.iter().sum::<f32>() / strategic_values.len() as f32;

    for _ in 1..config.num_players {
        let mut best_score = f32::NEG_INFINITY;
        let mut best = placed[0];

        for &(row, col) in &sampled {
            let ax = offset_to_axial(row as i32, col as i32);
            if placed.iter().any(|&p| p == ax) {
                continue;
            }
            let min_dist = placed
                .iter()
                .map(|&p| distance(p, ax) as f32)
                .fold(f32::INFINITY, f32::min);
            let sv = strategic_values[row * config.width + col];
            let sv_diff = (sv - avg_sv).abs();
            let score = min_dist - sv_diff * 0.05;
            if score > best_score {
                best_score = score;
                best = ax;
            }
        }
        placed.push(best);
    }

    placed
}

#[cfg(test)]
mod tests {
    use super::super::hex::distance;
    use super::*;

    fn default_state() -> GameState {
        generate(&MapConfig::default())
    }

    #[test]
    fn terrain_values_in_range() {
        let state = default_state();
        for cell in &state.grid {
            assert!(
                cell.terrain_value >= 0.0 && cell.terrain_value <= 3.0,
                "terrain_value {} out of range",
                cell.terrain_value
            );
        }
    }

    #[test]
    fn material_values_in_range() {
        let state = default_state();
        for cell in &state.grid {
            assert!(
                cell.material_value >= 0.0 && cell.material_value <= 2.0,
                "material_value {} out of range",
                cell.material_value
            );
        }
    }

    #[test]
    fn different_seeds_differ() {
        let s1 = generate(&MapConfig {
            seed: 1,
            ..Default::default()
        });
        let s2 = generate(&MapConfig {
            seed: 2,
            ..Default::default()
        });
        let diffs = s1
            .grid
            .iter()
            .zip(s2.grid.iter())
            .filter(|(a, b)| (a.terrain_value - b.terrain_value).abs() > 0.001)
            .count();
        assert!(
            diffs > 100,
            "seeds 1 and 2 produce too similar terrain ({} diffs)",
            diffs
        );
    }

    #[test]
    fn terrain_has_variance() {
        let state = default_state();
        let values: Vec<f32> = state.grid.iter().map(|c| c.terrain_value).collect();
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32;
        let std_dev = variance.sqrt();
        assert!(std_dev > 0.1, "std dev {} too low", std_dev);
    }

    #[test]
    fn material_available_on_map() {
        let state = default_state();
        let total_material: f32 = state.grid.iter().map(|c| c.material_value).sum();
        assert!(total_material > 0.0, "map should generate material");
    }

    #[test]
    fn generals_placed_on_valid_terrain() {
        let state = default_state();
        for player in &state.players {
            let general = state
                .units
                .iter()
                .find(|u| u.id == player.general_id)
                .unwrap();
            let cell = state.cell_at(general.pos).unwrap();
            assert!(
                cell.terrain_value > 1.0,
                "general at terrain_value {}",
                cell.terrain_value
            );
        }
    }

    #[test]
    fn generals_far_apart() {
        let state = default_state();
        let generals: Vec<_> = state
            .players
            .iter()
            .map(|p| {
                state
                    .units
                    .iter()
                    .find(|u| u.id == p.general_id)
                    .unwrap()
                    .pos
            })
            .collect();
        for i in 0..generals.len() {
            for j in (i + 1)..generals.len() {
                let d = distance(generals[i], generals[j]);
                let min_dist = MapConfig::default().width as i32 / 4;
                assert!(d > min_dist, "generals only {} apart (min {})", d, min_dist);
            }
        }
    }

    #[test]
    fn strategic_values_balanced() {
        let config = MapConfig::default();
        let state = generate(&config);
        let sv = compute_strategic_values(&state.grid, config.width, config.height);
        let gen_svs: Vec<f32> = state
            .players
            .iter()
            .map(|p| {
                let g = state.units.iter().find(|u| u.id == p.general_id).unwrap();
                let (row, col) = axial_to_offset(g.pos);
                sv[(row as usize) * config.width + (col as usize)]
            })
            .collect();
        let max_sv = gen_svs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min_sv = gen_svs.iter().cloned().fold(f32::INFINITY, f32::min);
        // Within 30%
        assert!(
            (max_sv - min_sv) / max_sv < 0.30,
            "strategic value imbalance: max={max_sv}, min={min_sv}"
        );
    }

    #[test]
    fn each_player_has_correct_unit_count() {
        let state = default_state();
        for player in &state.players {
            let count = state.units.iter().filter(|u| u.owner == player.id).count();
            // INITIAL_UNITS normal units + 1 general
            assert_eq!(
                count,
                INITIAL_UNITS + 1,
                "player {} has {} units",
                player.id,
                count
            );
        }
    }

    #[test]
    fn initial_units_near_general() {
        let state = default_state();
        for player in &state.players {
            let general_pos = state
                .units
                .iter()
                .find(|u| u.id == player.general_id)
                .unwrap()
                .pos;
            for unit in state
                .units
                .iter()
                .filter(|u| u.owner == player.id && !u.is_general)
            {
                let d = distance(general_pos, unit.pos);
                assert!(
                    d <= 3,
                    "unit for player {} is distance {} from general",
                    player.id,
                    d
                );
            }
        }
    }

    #[test]
    fn grid_size_matches_config() {
        let config = MapConfig {
            width: 20,
            height: 30,
            num_players: 2,
            seed: 99,
        };
        let state = generate(&config);
        assert_eq!(state.width, 20);
        assert_eq!(state.height, 30);
        assert_eq!(state.grid.len(), 600);
    }
}
