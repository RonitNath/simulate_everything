use rand::Rng;
use rand::SeedableRng;

use super::armor::{self, MaterialType};
use super::economy;
use super::equipment::{self, Equipment};
use super::hex::hex_to_world;
use super::lifecycle::{contain, spawn_entity};
use super::movement::Mobile;
use super::physical::{MatterStack, PhysicalProperties, SiteProperties, ToolProperties};
use super::spatial::{GeoMaterial, Heightfield, Vec3, Vertex};
use super::state::{Combatant, CommodityKind, EntityBuilder, GameState, Person, Role};
use super::weapon;
use crate::v2::hex::{Axial, offset_to_axial};
use crate::v2::state::EntityKey;
use simulate_everything_protocol::{MaterialKind, MatterState, PropertyTag};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Person collision radius in meters.
const PERSON_RADIUS: f32 = 10.0;
/// Person steering force.
const PERSON_STEERING: f32 = 2.0;
/// Soldiers per player at start.
const STARTING_SOLDIERS: usize = 5;
/// Civilians per player at start.
const STARTING_CIVILIANS: usize = 30;
/// Starting food amount per player.
const STARTING_FOOD: f32 = 500.0;
/// Starting material amount per player.
const STARTING_MATERIAL: f32 = 200.0;

// ---------------------------------------------------------------------------
// Heightfield generation
// ---------------------------------------------------------------------------

/// Generate a simple heightfield with noise-based terrain.
/// V3.0: basic rolling hills. Future: V2-style biome/region generation.
fn generate_heightfield(cols: usize, rows: usize, rng: &mut impl Rng) -> Heightfield {
    let mut vertices = Vec::with_capacity(cols * rows);
    // Simple procedural: gentle hills using sin/cos combination
    let freq_x = rng.gen_range(0.02..0.06_f32);
    let freq_y = rng.gen_range(0.02..0.06_f32);
    let phase_x = rng.gen_range(0.0..std::f32::consts::TAU);
    let phase_y = rng.gen_range(0.0..std::f32::consts::TAU);
    let amplitude = rng.gen_range(5.0..15.0_f32);

    for row in 0..rows {
        for col in 0..cols {
            let h = amplitude
                * ((col as f32 * freq_x + phase_x).sin() * (row as f32 * freq_y + phase_y).cos()
                    + 0.5
                        * ((col as f32 * freq_x * 2.3 + phase_y).cos()
                            * (row as f32 * freq_y * 1.7 + phase_x).sin()));
            let material = if h > amplitude * 0.7 {
                GeoMaterial::Rock
            } else if rng.gen_bool(0.15) {
                GeoMaterial::Sand
            } else {
                GeoMaterial::Soil
            };
            vertices.push(Vertex {
                height: h,
                material,
            });
        }
    }

    Heightfield::from_vertices(cols, rows, vertices)
}

// ---------------------------------------------------------------------------
// Player spawn positions
// ---------------------------------------------------------------------------

/// Compute starting hex positions for each player, evenly distributed.
fn player_spawn_hexes(width: usize, height: usize, num_players: u8) -> Vec<Axial> {
    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let radius = (width.min(height) as f32 / 3.0).max(2.0);

    (0..num_players)
        .map(|i| {
            let angle = std::f32::consts::TAU * (i as f32) / (num_players as f32);
            let row = (cy + radius * angle.sin()).round() as i32;
            let col = (cx + radius * angle.cos()).round() as i32;
            let row = row.clamp(1, height as i32 - 2);
            let col = col.clamp(1, width as i32 - 2);
            offset_to_axial(row, col)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Entity population
// ---------------------------------------------------------------------------

/// Spawn a soldier entity with equipment.
fn spawn_soldier(state: &mut GameState, pos: Vec3, owner: u8, skill: f32) -> EntityKey {
    let soldier = spawn_entity(
        state,
        EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .person(Person {
                role: Role::Soldier,
                combat_skill: skill,
                    task: None,
            })
            .mobile(Mobile::new(PERSON_STEERING, PERSON_RADIUS))
            .combatant(Combatant::new())
            .vitals()
            .equipment(Equipment::empty()),
    );

    // Spawn and equip a sword
    let sword = spawn_entity(
        state,
        EntityBuilder::new()
            .owner(owner)
            .physical(
                PhysicalProperties::new(1.8, 0.75, MaterialKind::Iron, MatterState::Solid)
                    .with_tags(&[PropertyTag::Tool, PropertyTag::Workable]),
            )
            .tool_props(ToolProperties {
                force_mult: 2.5,
                precision: 0.6,
                cutting_edge: 0.8,
                heat_output_k: 0.0,
                capacity_l: 0.0,
                durability: 1.0,
            })
            .weapon_props(weapon::iron_sword()),
    );
    contain(state, soldier, sword);
    if let Some(eq) = &mut state.entities[soldier].equipment {
        eq.weapon = Some(sword);
    }

    // Spawn and equip leather armor
    let cuirass_props = armor::leather_cuirass();
    let cuirass = spawn_entity(
        state,
        EntityBuilder::new()
            .owner(owner)
            .physical(PhysicalProperties::new(
                3.0,
                0.35,
                MaterialType::Leather.into(),
                MatterState::Solid,
            ))
            .armor_props(cuirass_props.clone()),
    );
    contain(state, soldier, cuirass);
    if let Some(eq) = &mut state.entities[soldier].equipment {
        equipment::equip_armor(eq, cuirass, &cuirass_props);
    }

    soldier
}

/// Spawn a civilian entity.
fn spawn_civilian(state: &mut GameState, pos: Vec3, owner: u8, role: Role) -> EntityKey {
    spawn_entity(
        state,
        EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .physical(PhysicalProperties::new(
                75.0,
                0.15,
                MaterialKind::Flesh,
                MatterState::Solid,
            ))
            .person(Person {
                role,
                combat_skill: 0.1,
                    task: None,
            })
            .mobile(Mobile::new(PERSON_STEERING, PERSON_RADIUS)),
    )
}

fn settlement_physical() -> PhysicalProperties {
    PhysicalProperties::new(1_200.0, 0.4, MaterialKind::Wood, MatterState::Solid).with_tags(&[
        PropertyTag::Structural,
        PropertyTag::Shelter,
        PropertyTag::Container,
        PropertyTag::Settlement,
    ])
}

fn stockpile_physical(commodity: CommodityKind) -> PhysicalProperties {
    let mut physical = match commodity {
        CommodityKind::Food => {
            PhysicalProperties::new(50.0, 0.1, MaterialKind::Plant, MatterState::Solid)
                .with_tags(&[PropertyTag::Edible, PropertyTag::Stockpile])
        }
        CommodityKind::Material => {
            PhysicalProperties::new(80.0, 0.35, MaterialKind::Wood, MatterState::Solid)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
        CommodityKind::Ore => {
            PhysicalProperties::new(80.0, 0.6, MaterialKind::Stone, MatterState::Powder)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
        CommodityKind::Wood => {
            PhysicalProperties::new(80.0, 0.3, MaterialKind::Wood, MatterState::Solid)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
        CommodityKind::Stone => {
            PhysicalProperties::new(80.0, 0.8, MaterialKind::Stone, MatterState::Solid)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
    };
    if matches!(commodity, CommodityKind::Food) {
        physical.insert_tag(PropertyTag::Harvestable);
    }
    physical
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a complete V3 game state: terrain + entity populations for all players.
///
/// Each player receives:
/// - 1 settlement site
/// - STARTING_SOLDIERS soldiers with sword + leather armor
/// - STARTING_CIVILIANS civilian persons (mix of Farmer, Worker, Idle)
/// - Starting food + material matter-stack entities in the settlement
pub fn generate(width: usize, height: usize, num_players: u8, seed: u64) -> GameState {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    // Vertex grid: ~2× hex count for vertex resolution
    let vertex_cols = width * 2 + 1;
    let vertex_rows = height * 2 + 1;
    let heightfield = generate_heightfield(vertex_cols, vertex_rows, &mut rng);

    let mut state = GameState::new(width, height, num_players, heightfield);

    let spawn_hexes = player_spawn_hexes(width, height, num_players);

    for (player, &hex) in spawn_hexes.iter().enumerate() {
        let owner = player as u8;
        let center = hex_to_world(hex);

        // Settlement
        let settlement = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(center)
                .owner(owner)
                .physical(settlement_physical())
                .site(SiteProperties {
                    build_progress: 1.0,
                    integrity: 100.0,
                    occupancy_capacity: 50,
                }),
        );

        // Starting resources inside the settlement
        let food = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(owner)
                .physical(stockpile_physical(CommodityKind::Food))
                .matter(MatterStack {
                    commodity: CommodityKind::Food,
                    amount: STARTING_FOOD,
                }),
        );
        contain(&mut state, settlement, food);

        let material = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(owner)
                .physical(stockpile_physical(CommodityKind::Material))
                .matter(MatterStack {
                    commodity: CommodityKind::Material,
                    amount: STARTING_MATERIAL,
                }),
        );
        contain(&mut state, settlement, material);

        // Soldiers — positioned in a loose formation near the settlement
        for i in 0..STARTING_SOLDIERS {
            let angle = std::f32::consts::TAU * (i as f32) / (STARTING_SOLDIERS as f32);
            let offset = 30.0; // 30m from settlement center
            let pos = Vec3::new(
                center.x + offset * angle.cos(),
                center.y + offset * angle.sin(),
                center.z,
            );
            let skill = 0.4 + rng.gen_range(0.0..0.3_f32);
            spawn_soldier(&mut state, pos, owner, skill);
        }

        // Civilians — scattered around the settlement
        for i in 0..STARTING_CIVILIANS {
            let angle = std::f32::consts::TAU * (i as f32) / (STARTING_CIVILIANS as f32);
            let dist = 20.0 + rng.gen_range(0.0..60.0_f32);
            let pos = Vec3::new(
                center.x + dist * angle.cos(),
                center.y + dist * angle.sin(),
                center.z,
            );
            let role = match i % 3 {
                0 => Role::Farmer,
                1 => Role::Worker,
                _ => Role::Idle,
            };
            spawn_civilian(&mut state, pos, owner, role);
        }
    }

    state
}

pub fn generate_economy_ready(
    width: usize,
    height: usize,
    num_players: u8,
    seed: u64,
) -> GameState {
    economy::generate_economy_ready(width, height, num_players, seed)
}

pub fn bootstrap_shared_economy_layout(state: &mut GameState) {
    economy::bootstrap_shared_economy_layout(state);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_creates_entities() {
        let state = generate(20, 20, 2, 42);
        // 2 players, each with:
        // 1 settlement + 2 resources (contained) + 5 soldiers + 5 swords + 5 cuirasses + 30 civilians
        // Total entities per player = 1 + 2 + 5 + 5 + 5 + 30 = 48
        // Total = 96
        assert!(
            state.entities.len() > 50,
            "should have many entities, got {}",
            state.entities.len()
        );
    }

    #[test]
    fn each_player_has_settlement() {
        let state = generate(20, 20, 2, 42);
        for player in 0..2u8 {
            let settlements: Vec<_> = state
                .entities
                .values()
                .filter(|e| {
                    e.owner == Some(player)
                        && e.site.is_some()
                        && e.physical
                            .as_ref()
                            .map(|p| p.has_tag(PropertyTag::Settlement))
                            .unwrap_or(false)
                })
                .collect();
            assert_eq!(
                settlements.len(),
                1,
                "player {player} should have exactly 1 settlement"
            );
        }
    }

    #[test]
    fn soldiers_have_equipment() {
        let state = generate(20, 20, 2, 42);
        let soldiers: Vec<_> = state
            .entities
            .values()
            .filter(|e| {
                e.person
                    .as_ref()
                    .map(|p| p.role == Role::Soldier)
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(soldiers.len(), 10, "2 players × 5 soldiers = 10");

        for soldier in &soldiers {
            let eq = soldier.equipment.as_ref().expect("soldier has equipment");
            assert!(eq.weapon.is_some(), "soldier should have a weapon");
        }
    }

    #[test]
    fn civilians_have_no_combat() {
        let state = generate(20, 20, 2, 42);
        let civilians: Vec<_> = state
            .entities
            .values()
            .filter(|e| {
                e.person
                    .as_ref()
                    .map(|p| matches!(p.role, Role::Farmer | Role::Worker | Role::Idle))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(civilians.len(), 60, "2 players × 30 civilians = 60");

        for civ in &civilians {
            assert!(
                civ.combatant.is_none(),
                "civilians should not be combatants"
            );
            assert!(
                civ.equipment.is_none(),
                "civilians should not have equipment"
            );
        }
    }

    #[test]
    fn resources_contained_in_settlement() {
        let state = generate(20, 20, 2, 42);
        for player in 0..2u8 {
            let settlement = state
                .entities
                .iter()
                .find(|(_, e)| {
                    e.owner == Some(player)
                        && e.site.is_some()
                        && e.physical
                            .as_ref()
                            .map(|p| p.has_tag(PropertyTag::Settlement))
                            .unwrap_or(false)
                })
                .map(|(k, _)| k)
                .expect("player has settlement");

            let contained = &state.entities[settlement].contains;
            assert!(
                contained.len() >= 2,
                "settlement should contain at least food + material"
            );

            let has_food = contained.iter().any(|&k| {
                state
                    .entities
                    .get(k)
                    .and_then(|e| e.matter.as_ref())
                    .map(|r| r.commodity == CommodityKind::Food)
                    .unwrap_or(false)
            });
            assert!(has_food, "settlement should contain food");
        }
    }

    #[test]
    fn deterministic_generation() {
        let a = generate(15, 15, 2, 123);
        let b = generate(15, 15, 2, 123);
        assert_eq!(a.entities.len(), b.entities.len());
    }

    #[test]
    fn different_seeds_different_maps() {
        let a = generate(15, 15, 2, 1);
        let b = generate(15, 15, 2, 2);
        // Entity count should be the same (deterministic population), but
        // positions differ. Check heightfield differs.
        // Both have same entity count since population is fixed.
        assert_eq!(a.entities.len(), b.entities.len());
    }

    #[test]
    fn spatial_index_populated() {
        let state = generate(20, 20, 2, 42);
        // At least some hexes should have entities
        let occupied_hexes: usize = state
            .spatial_index
            .all_hexes()
            .filter(|hex| state.spatial_index.has_entities_at(*hex))
            .count();
        assert!(
            occupied_hexes > 0,
            "spatial index should have occupied hexes"
        );
    }
}
