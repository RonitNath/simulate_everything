use crate::v2::hex::{Axial, axial_to_offset, offset_to_axial};

use super::state::{CommodityKind, GameState, Role};
use simulate_everything_protocol::PropertyTag;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexControl {
    pub owner: Option<u8>,
    pub contested: bool,
}

#[derive(Debug, Clone)]
pub struct PlayerDerivedStats {
    pub id: u8,
    pub population: u32,
    pub soldiers: u32,
    pub farmers: u32,
    pub workers: u32,
    pub idle: u32,
    pub workshops: u32,
    pub settlements: u32,
    pub territory: u32,
    pub food_stockpile: f32,
    pub material_stockpile: f32,
    pub alive: bool,
}

pub fn derive_hex_control(state: &GameState) -> Vec<HexControl> {
    let hex_count = state.map_width * state.map_height;
    let mut counts = vec![vec![0u16; state.num_players as usize]; hex_count];

    for entity in state.entities.values() {
        if entity.person.is_none() {
            continue;
        }
        if entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false) {
            continue;
        }
        let Some(owner) = entity.owner else {
            continue;
        };
        let Some(hex) = entity.hex else {
            continue;
        };
        let Some(idx) = hex_index(state, hex) else {
            continue;
        };
        counts[idx][owner as usize] += 1;
    }

    counts
        .into_iter()
        .map(|owners| {
            let occupied_players = owners.iter().filter(|&&count| count > 0).count();
            let owner = owners
                .iter()
                .enumerate()
                .max_by_key(|(_, count)| **count)
                .and_then(|(player, &count)| (count > 0).then_some(player as u8));

            HexControl {
                owner,
                contested: occupied_players > 1,
            }
        })
        .collect()
}

pub fn derive_hex_structures(state: &GameState) -> Vec<Option<u32>> {
    let hex_count = state.map_width * state.map_height;
    let mut structures: Vec<Option<(u32, f32)>> = vec![None; hex_count];

    for entity in state.entities.values() {
        let Some(site) = entity.site.as_ref() else {
            continue;
        };
        let Some(hex) = entity.hex else {
            continue;
        };
        let Some(idx) = hex_index(state, hex) else {
            continue;
        };

        let candidate = (entity.id, site.build_progress);
        let replace = match structures[idx] {
            Some((existing_id, existing_progress)) => {
                candidate.1 > existing_progress
                    || (candidate.1 == existing_progress && candidate.0 > existing_id)
            }
            None => true,
        };
        if replace {
            structures[idx] = Some(candidate);
        }
    }

    structures
        .into_iter()
        .map(|entry| entry.map(|(id, _)| id))
        .collect()
}

pub fn derive_player_stats(state: &GameState) -> Vec<PlayerDerivedStats> {
    let hex_control = derive_hex_control(state);
    let mut territory = vec![0u32; state.num_players as usize];
    for hex in &hex_control {
        if let Some(owner) = hex.owner {
            territory[owner as usize] += 1;
        }
    }

    let mut stats: Vec<PlayerDerivedStats> = (0..state.num_players)
        .map(|id| PlayerDerivedStats {
            id,
            population: 0,
            soldiers: 0,
            farmers: 0,
            workers: 0,
            idle: 0,
            workshops: 0,
            settlements: 0,
            territory: territory[id as usize],
            food_stockpile: 0.0,
            material_stockpile: 0.0,
            alive: false,
        })
        .collect();

    for entity in state.entities.values() {
        let Some(owner) = entity.owner else {
            continue;
        };
        let player = &mut stats[owner as usize];

        if let Some(person) = entity.person.as_ref() {
            player.population += 1;
            player.alive |= entity.vitals.as_ref().map(|v| !v.is_dead()).unwrap_or(true);
            match person.role {
                Role::Soldier => player.soldiers += 1,
                Role::Farmer => player.farmers += 1,
                Role::Worker | Role::Builder => player.workers += 1,
                Role::Idle => player.idle += 1,
            }
        }

        if let Some(resource) = entity.matter.as_ref() {
            match resource.commodity {
                CommodityKind::Food => player.food_stockpile += resource.amount,
                CommodityKind::Material
                | CommodityKind::Ore
                | CommodityKind::Wood
                | CommodityKind::Stone => player.material_stockpile += resource.amount,
            }
        }

        if entity.site.is_some()
            && let Some(physical) = entity.physical.as_ref()
        {
            if physical.has_tag(PropertyTag::Workshop) {
                player.workshops += 1;
            }
            if physical.has_tag(PropertyTag::Settlement) || physical.has_tag(PropertyTag::Farm) {
                player.settlements += 1;
            }
        }
    }

    stats
}

pub fn region_center(state: &GameState, hexes: &[usize]) -> Axial {
    if hexes.is_empty() {
        return Axial::new(0, 0);
    }

    let (sum_row, sum_col) = hexes.iter().fold((0.0f32, 0.0f32), |(rows, cols), &idx| {
        let row = idx / state.map_width;
        let col = idx % state.map_width;
        (rows + row as f32, cols + col as f32)
    });

    let count = hexes.len() as f32;
    offset_to_axial(
        (sum_row / count).round() as i32,
        (sum_col / count).round() as i32,
    )
}

pub fn stockpile_level(amount: f32) -> u8 {
    (amount / 25.0).floor().clamp(0.0, u8::MAX as f32) as u8
}

fn hex_index(state: &GameState, hex: Axial) -> Option<usize> {
    let (row, col) = axial_to_offset(hex);
    if row < 0 || col < 0 {
        return None;
    }
    let row = row as usize;
    let col = col as usize;
    if row >= state.map_height || col >= state.map_width {
        return None;
    }
    Some(row * state.map_width + col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::state::EntityKey;

    use super::super::formation::FormationType;
    use super::super::lifecycle::spawn_entity;
    use super::super::movement::Mobile;
    use super::super::physical::{MatterStack, PhysicalProperties, SiteProperties};
    use super::super::spatial::{GeoMaterial, Heightfield, Vec3};
    use super::super::state::{Combatant, CommodityKind, EntityBuilder, Person, Stack};
    use simulate_everything_protocol::{MaterialKind, MatterState, PropertyTag};
    use smallvec::SmallVec;

    fn test_state() -> GameState {
        let hf = Heightfield::new(10, 10, 0.0, GeoMaterial::Soil);
        GameState::new(10, 10, 2, hf)
    }

    fn spawn_person(state: &mut GameState, pos: Vec3, owner: u8, role: Role) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(Person {
                    role,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals(),
        )
    }

    #[test]
    fn derives_hex_control_and_contested_state() {
        let mut state = test_state();
        spawn_person(&mut state, Vec3::new(0.0, 0.0, 0.0), 0, Role::Soldier);
        spawn_person(&mut state, Vec3::new(0.0, 0.0, 0.0), 1, Role::Soldier);
        spawn_person(&mut state, Vec3::new(40.0, 0.0, 0.0), 0, Role::Soldier);

        let control = derive_hex_control(&state);
        assert!(control.iter().any(|hex| hex.contested));
        assert!(control.iter().any(|hex| hex.owner == Some(0)));
    }

    #[test]
    fn derives_player_stats_and_structures() {
        let mut state = test_state();
        let leader = spawn_person(&mut state, Vec3::new(0.0, 0.0, 0.0), 0, Role::Soldier);
        spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(0.0, 0.0, 0.0))
                .owner(0)
                .physical(
                    PhysicalProperties::new(50.0, 0.1, MaterialKind::Plant, MatterState::Solid)
                        .with_tags(&[PropertyTag::Edible, PropertyTag::Stockpile]),
                )
                .matter(MatterStack {
                    commodity: CommodityKind::Food,
                    amount: 75.0,
                }),
        );
        spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(20.0, 0.0, 0.0))
                .owner(0)
                .physical(
                    PhysicalProperties::new(900.0, 0.4, MaterialKind::Wood, MatterState::Solid)
                        .with_tags(&[
                            PropertyTag::Workshop,
                            PropertyTag::Structural,
                            PropertyTag::Container,
                        ]),
                )
                .site(SiteProperties {
                    build_progress: 1.0,
                    integrity: 100.0,
                    occupancy_capacity: 10,
                }),
        );
        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: SmallVec::from_slice(&[leader]),
            formation: FormationType::Line,
            leader,
        });

        let stats = derive_player_stats(&state);
        assert_eq!(stats[0].population, 1);
        assert_eq!(stats[0].soldiers, 1);
        assert_eq!(stats[0].workshops, 1);
        assert_eq!(stats[0].food_stockpile, 75.0);

        let structures = derive_hex_structures(&state);
        assert!(structures.iter().any(|id| id.is_some()));
    }
}
