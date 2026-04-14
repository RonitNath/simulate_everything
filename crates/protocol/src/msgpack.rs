use serde::{Deserialize, Serialize};

/// Encode a value as MessagePack bytes.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec_named(value)
}

/// Decode MessagePack bytes into a value.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn round_trip_init() {
        let init = V3Init {
            width: 20,
            height: 20,
            terrain: vec![0.5; 400],
            height_map: vec![1.0; 400],
            material_map: vec![0.2; 400],
            terrain_raster: TerrainRasterInit {
                width: 32,
                height: 32,
                origin_x: -10.0,
                origin_y: -10.0,
                cell_size: 1.0,
                heights: vec![0.0; 1024],
                materials: vec![0; 1024],
            },
            region_ids: vec![0; 400],
            player_count: 2,
            agent_names: vec!["Alpha".into(), "Beta".into()],
            agent_versions: vec!["1.0".into(), "1.0".into()],
            game_number: 1,
        };
        let bytes = encode(&init).unwrap();
        let decoded: V3Init = decode(&bytes).unwrap();
        assert_eq!(decoded.width, 20);
        assert_eq!(decoded.player_count, 2);
    }

    #[test]
    fn round_trip_snapshot() {
        let snapshot = V3Snapshot {
            tick: 42,
            dt: 1.0,
            full_state: true,
            entities: vec![SpectatorEntityInfo {
                id: 1,
                owner: Some(0),
                x: 10.0,
                y: 20.0,
                z: 0.0,
                hex_q: 3,
                hex_r: 4,
                facing: Some(1.57),
                entity_kind: EntityKind::Person,
                role: Some(Role::Soldier),
                blood: Some(1.0),
                stamina: Some(0.8),
                wounds: vec![(BodyZone::Torso, WoundSeverity::Light)],
                weapon_type: Some("Iron Slash".into()),
                armor_type: None,
                resource_type: None,
                resource_amount: None,
                structure_type: None,
                build_progress: None,
                contains_count: 0,
                stack_id: None,
                current_task: Some("Move".into()),
                attack_phase: None,
                attack_motion: None,
                weapon_angle: None,
                attack_progress: None,
            }],
            body_models: vec![BodyRenderInfo {
                entity_id: 1,
                points: [BodyPointWire {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }; 16],
                weapon: Some(CapsuleWire {
                    a: [0.0, 0.0, 0.0],
                    b: [1.0, 0.0, 0.0],
                    radius: 0.05,
                }),
                shield: None,
            }],
            projectiles: vec![],
            stacks: vec![],
            hex_ownership: vec![None; 4],
            hex_roads: vec![0; 4],
            hex_structures: vec![None; 4],
            players: vec![PlayerInfo {
                id: 0,
                population: 10,
                territory: 5,
                food_level: 2,
                material_level: 1,
                alive: true,
                score: 15,
            }],
        };
        let bytes = encode(&snapshot).unwrap();
        let decoded: V3Snapshot = decode(&bytes).unwrap();
        assert_eq!(decoded.tick, 42);
        assert_eq!(decoded.entities.len(), 1);
        assert_eq!(decoded.entities[0].role, Some(Role::Soldier));
        assert_eq!(decoded.body_models.len(), 1);
    }

    #[test]
    fn round_trip_server_message() {
        let msg = V3ServerToSpectator::GameEnd {
            winner: Some(0),
            tick: 500,
            timed_out: false,
            scores: vec![100, 50],
        };
        let bytes = encode(&msg).unwrap();
        let decoded: V3ServerToSpectator = decode(&bytes).unwrap();
        match decoded {
            V3ServerToSpectator::GameEnd { winner, tick, .. } => {
                assert_eq!(winner, Some(0));
                assert_eq!(tick, 500);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_delta() {
        let delta = V3SnapshotDelta {
            tick: 10,
            dt: 1.0,
            full_state: false,
            entities_appeared: vec![],
            entities_updated: vec![EntityUpdate {
                id: 1,
                x: Some(15.0),
                y: None,
                z: None,
                hex_q: None,
                hex_r: None,
                facing: None,
                blood: None,
                stamina: None,
                wounds: None,
                weapon_type: None,
                armor_type: None,
                contains_count: None,
                stack_id: None,
                current_task: None,
                attack_phase: None,
                attack_motion: None,
                weapon_angle: None,
                attack_progress: None,
            }],
            entities_removed: vec![],
            body_models_appeared: vec![],
            body_models_updated: vec![BodyRenderInfo {
                entity_id: 1,
                points: [BodyPointWire {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }; 16],
                weapon: None,
                shield: Some(DiscWire {
                    center: [0.0, 0.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                    radius: 0.4,
                }),
            }],
            body_models_removed: vec![],
            projectiles_spawned: vec![],
            projectiles_removed: vec![],
            stacks_created: vec![],
            stacks_updated: vec![],
            stacks_dissolved: vec![],
            hex_changes: vec![],
            terrain_patches: vec![TerrainPatch {
                x: 10,
                y: 20,
                width: 4,
                height: 4,
                heights: vec![1.0; 16],
                materials: vec![2; 16],
            }],
            players: vec![],
        };
        let bytes = encode(&delta).unwrap();
        let decoded: V3SnapshotDelta = decode(&bytes).unwrap();
        assert_eq!(decoded.entities_updated[0].x, Some(15.0));
        assert_eq!(decoded.terrain_patches.len(), 1);
        assert_eq!(decoded.body_models_updated.len(), 1);
    }
}
