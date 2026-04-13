use std::collections::HashMap;

use super::directive::Directive;
use super::hex::{self, Axial};
use super::observation::{Observation, UnitInfo};
use super::state::{CargoType, Role};
use super::{SOLDIERS_PER_UNIT, UNIT_FOOD_COST, UNIT_MATERIAL_COST};

pub trait Agent: Send {
    fn name(&self) -> &str;
    fn act(&mut self, obs: &Observation) -> Vec<Directive>;
    fn reset(&mut self) {}
}

pub struct SpreadAgent;

impl SpreadAgent {
    pub fn new() -> Self {
        Self
    }
}

impl Agent for SpreadAgent {
    fn name(&self) -> &str {
        "spread"
    }

    fn act(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();
        let general = obs.own_units.iter().find(|u| u.is_general);
        let general_hex = general.map(|u| Axial::new(u.q, u.r));

        if let Some(hex) = general_hex {
            if general_hex_needs_depot(obs, hex) {
                directives.push(Directive::BuildDepot {
                    hex_q: hex.q,
                    hex_r: hex.r,
                });
            }
            if general_hex_needs_road(obs, hex) {
                directives.push(Directive::BuildRoad {
                    hex_q: hex.q,
                    hex_r: hex.r,
                    level: 1,
                });
            }

            let (idle, farmers, workers, trained_soldiers, untrained_soldiers) =
                population_mix(obs, hex);
            let total = idle + farmers + workers + trained_soldiers + untrained_soldiers;
            let target_farmers = (total as f32 * 0.45).ceil() as u16;
            let target_workers = (total as f32 * 0.2).ceil() as u16;

            if farmers < target_farmers && idle > 0 {
                directives.push(Directive::AssignRole {
                    hex_q: hex.q,
                    hex_r: hex.r,
                    role: Role::Farmer,
                    count: (target_farmers - farmers).min(3),
                });
            } else if workers < target_workers && idle > 0 {
                directives.push(Directive::AssignRole {
                    hex_q: hex.q,
                    hex_r: hex.r,
                    role: Role::Worker,
                    count: (target_workers - workers).min(2),
                });
            } else if idle > 0 {
                directives.push(Directive::TrainSoldier {
                    hex_q: hex.q,
                    hex_r: hex.r,
                });
            }

            let general_idx = cell_index(obs, hex);
            if let Some(idx) = general_idx {
                let food = obs.food_stockpiles[idx];
                let material = obs.material_stockpiles[idx];
                let mut remaining_food = food;
                let mut remaining_material = material;
                let mut ready = trained_soldiers;
                while remaining_food >= UNIT_FOOD_COST
                    && remaining_material >= UNIT_MATERIAL_COST
                    && ready >= SOLDIERS_PER_UNIT
                {
                    directives.push(Directive::Produce);
                    remaining_food -= UNIT_FOOD_COST;
                    remaining_material -= UNIT_MATERIAL_COST;
                    ready -= SOLDIERS_PER_UNIT;
                }
            }

            for (idx, owner) in obs.stockpile_owner.iter().enumerate() {
                if *owner != Some(obs.player) {
                    continue;
                }
                let hex = index_to_hex(obs, idx);
                if Some(hex) == general_hex {
                    continue;
                }
                if obs.food_stockpiles[idx] > 15.0 {
                    directives.push(Directive::LoadConvoy {
                        hex_q: hex.q,
                        hex_r: hex.r,
                        cargo_type: CargoType::Food,
                        amount: 10.0,
                    });
                } else if obs.material_stockpiles[idx] > 10.0 {
                    directives.push(Directive::LoadConvoy {
                        hex_q: hex.q,
                        hex_r: hex.r,
                        cargo_type: CargoType::Material,
                        amount: 10.0,
                    });
                }
            }
        }

        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<u32, usize> = count_friendlies_near_enemies(obs);

        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            if !unit.engagements.is_empty() {
                continue;
            }
            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }
            if unit.is_general {
                if obs.own_units.len() > 10 {
                    let gen_pos = Axial::new(unit.q, unit.r);
                    if hex::distance(gen_pos, map_center) > 5 {
                        directives.push(Directive::Move {
                            unit_id: unit.id,
                            q: map_center.q,
                            r: map_center.r,
                        });
                    }
                }
                continue;
            }

            let dest = if obs.own_units.len() <= 8 {
                pick_sector_destination(unit, idx, obs, map_center)
            } else {
                pick_lane_destination(unit, obs, enemy_target)
            };
            if let Some(d) = dest {
                directives.push(Directive::Move {
                    unit_id: unit.id,
                    q: d.q,
                    r: d.r,
                });
                if let Some(idx) = cell_index(obs, Axial::new(unit.q, unit.r)) {
                    if obs.stockpile_owner[idx] == Some(obs.player) && obs.road_levels[idx] == 0 {
                        directives.push(Directive::BuildRoad {
                            hex_q: unit.q,
                            hex_r: unit.r,
                            level: 1,
                        });
                    }
                }
            }
        }

        directives
    }
}

fn cell_index(obs: &Observation, ax: Axial) -> Option<usize> {
    let (row, col) = hex::axial_to_offset(ax);
    if row < 0 || col < 0 {
        return None;
    }
    let (row, col) = (row as usize, col as usize);
    if row < obs.height && col < obs.width {
        Some(row * obs.width + col)
    } else {
        None
    }
}

fn index_to_hex(obs: &Observation, idx: usize) -> Axial {
    let row = idx / obs.width;
    let col = idx % obs.width;
    hex::offset_to_axial(row as i32, col as i32)
}

fn general_hex_needs_depot(obs: &Observation, hex: Axial) -> bool {
    cell_index(obs, hex)
        .map(|idx| obs.material_stockpiles[idx] >= 20.0)
        .unwrap_or(false)
}

fn general_hex_needs_road(obs: &Observation, hex: Axial) -> bool {
    cell_index(obs, hex)
        .map(|idx| obs.road_levels[idx] == 0)
        .unwrap_or(false)
}

fn population_mix(obs: &Observation, hex: Axial) -> (u16, u16, u16, u16, u16) {
    let mut idle = 0;
    let mut farmers = 0;
    let mut workers = 0;
    let mut trained_soldiers = 0;
    let mut untrained_soldiers = 0;
    for pop in obs
        .own_population
        .iter()
        .filter(|p| p.q == hex.q && p.r == hex.r)
    {
        match pop.role {
            Role::Idle => idle += pop.count,
            Role::Farmer => farmers += pop.count,
            Role::Worker => workers += pop.count,
            Role::Soldier => {
                if pop.training >= 1.0 {
                    trained_soldiers += pop.count;
                } else {
                    untrained_soldiers += pop.count;
                }
            }
        }
    }
    (idle, farmers, workers, trained_soldiers, untrained_soldiers)
}

fn count_friendlies_near_enemies(obs: &Observation) -> HashMap<u32, usize> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for enemy in &obs.visible_enemies {
        let enemy_pos = Axial::new(enemy.q, enemy.r);
        let count = obs
            .own_units
            .iter()
            .filter(|u| !u.is_general && u.engagements.is_empty())
            .filter(|u| hex::distance(Axial::new(u.q, u.r), enemy_pos) <= 1)
            .count();
        counts.insert(enemy.id, count);
    }
    counts
}

fn find_engageable_enemy(
    unit: &UnitInfo,
    enemy_by_pos: &HashMap<(i32, i32), &UnitInfo>,
    friendly_counts: &HashMap<u32, usize>,
) -> Option<u32> {
    let unit_pos = Axial::new(unit.q, unit.r);
    hex::neighbors(unit_pos)
        .iter()
        .filter_map(|nb| enemy_by_pos.get(&(nb.q, nb.r)).copied())
        .filter(|e| {
            let friends = friendly_counts.get(&e.id).copied().unwrap_or(0);
            friends >= 2 || unit.strength >= e.strength * 0.5
        })
        .min_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
        .map(|e| e.id)
}

fn pick_sector_destination(
    unit: &UnitInfo,
    idx: usize,
    obs: &Observation,
    map_center: Axial,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);
    let nearby_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 8)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = nearby_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    let non_general_count = obs
        .own_units
        .iter()
        .filter(|u| !u.is_general)
        .count()
        .max(1);
    let non_gen_idx = obs
        .own_units
        .iter()
        .filter(|u| !u.is_general)
        .position(|u| u.id == unit.id)
        .unwrap_or(idx);

    let angle = (non_gen_idx as f32 / non_general_count as f32) * std::f32::consts::TAU;
    let spread_radius = (obs.width.min(obs.height) / 4) as f32;
    let target_row = (map_center.r as f32 + angle.sin() * spread_radius) as i32;
    let target_col = {
        let (_cr, cc) = hex::axial_to_offset(map_center);
        (cc as f32 + angle.cos() * spread_radius) as i32
    };
    Some(hex::offset_to_axial(
        target_row.clamp(1, obs.height as i32 - 2),
        target_col.clamp(1, obs.width as i32 - 2),
    ))
}

fn pick_lane_destination(
    unit: &UnitInfo,
    obs: &Observation,
    enemy_target: Option<Axial>,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);
    if let Some(enemy) = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 10)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)))
    {
        return Some(Axial::new(enemy.q, enemy.r));
    }
    let target = enemy_target?;
    let (target_r, target_c) = hex::axial_to_offset(target);
    let (unit_r, unit_c) = hex::axial_to_offset(unit_pos);
    let dx = target_c - unit_c;
    let dy = target_r - unit_r;
    let lane_hash = ((unit.id as i32 * 7 + 13) % 5) - 2;
    let perp_offset = lane_hash * 3;
    let dest_r = target_r
        + if dx != 0 {
            perp_offset * dx.signum()
        } else {
            0
        };
    let dest_c = target_c
        + if dy != 0 {
            perp_offset * dy.signum()
        } else {
            0
        };
    Some(hex::offset_to_axial(
        dest_r.clamp(1, obs.height as i32 - 2),
        dest_c.clamp(1, obs.width as i32 - 2),
    ))
}

fn enemy_direction(obs: &Observation) -> Option<Axial> {
    if let Some(enemy_gen) = obs.visible_enemies.iter().find(|e| e.is_general) {
        return Some(Axial::new(enemy_gen.q, enemy_gen.r));
    }
    if !obs.visible_enemies.is_empty() {
        let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
        let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
        let n = obs.visible_enemies.len() as i32;
        return Some(Axial::new(sum_q / n, sum_r / n));
    }
    let own_gen = obs.own_units.iter().find(|u| u.is_general)?;
    let (gen_row, gen_col) = hex::axial_to_offset(Axial::new(own_gen.q, own_gen.r));
    Some(hex::offset_to_axial(
        ((obs.height as i32 - 1) - gen_row).clamp(0, obs.height as i32 - 1),
        ((obs.width as i32 - 1) - gen_col).clamp(0, obs.width as i32 - 1),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::mapgen::{MapConfig, generate};
    use crate::v2::observation::observe;

    #[test]
    fn spread_agent_manages_population() {
        let state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);
        assert!(directives.iter().any(|d| {
            matches!(
                d,
                Directive::AssignRole { .. }
                    | Directive::TrainSoldier { .. }
                    | Directive::BuildDepot { .. }
            )
        }));
    }

    #[test]
    fn spread_agent_moves_idle_units() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);
        assert!(
            directives
                .iter()
                .any(|d| matches!(d, Directive::Move { .. }))
        );
    }
}
