use serde::{Deserialize, Serialize};

use crate::v2::state::EntityKey;

/// Explicit melee motions for swordplay. These are carried through impacts so
/// benchmarks and review artifacts can reason about what actually happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackMotion {
    Generic,
    Overhead,
    Forehand,
    Backhand,
    Thrust,
}

/// Reactive sword/shield responses. Training affects how often a defender
/// chooses the right one for the incoming motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockManeuver {
    Generic,
    HighGuard,
    InsideParry,
    OutsideParry,
    LowGuard,
}

#[derive(Debug, Clone, Copy)]
pub struct AttackMotionProfile {
    pub windup_scale: f32,
    pub force_scale: f32,
    pub precision_scale: f32,
}

impl AttackMotion {
    pub fn short_name(self) -> &'static str {
        match self {
            AttackMotion::Generic => "GEN",
            AttackMotion::Overhead => "OVH",
            AttackMotion::Forehand => "FOR",
            AttackMotion::Backhand => "BAC",
            AttackMotion::Thrust => "THR",
        }
    }
}

impl BlockManeuver {
    pub fn short_name(self) -> &'static str {
        match self {
            BlockManeuver::Generic => "GEN",
            BlockManeuver::HighGuard => "HIG",
            BlockManeuver::InsideParry => "IN",
            BlockManeuver::OutsideParry => "OUT",
            BlockManeuver::LowGuard => "LOW",
        }
    }
}

pub fn attack_motion_profile(motion: AttackMotion) -> AttackMotionProfile {
    match motion {
        AttackMotion::Generic => AttackMotionProfile {
            windup_scale: 1.0,
            force_scale: 1.0,
            precision_scale: 1.0,
        },
        AttackMotion::Overhead => AttackMotionProfile {
            windup_scale: 1.15,
            force_scale: 1.12,
            precision_scale: 1.08,
        },
        AttackMotion::Forehand => AttackMotionProfile {
            windup_scale: 1.0,
            force_scale: 1.0,
            precision_scale: 0.95,
        },
        AttackMotion::Backhand => AttackMotionProfile {
            windup_scale: 0.88,
            force_scale: 0.9,
            precision_scale: 1.05,
        },
        AttackMotion::Thrust => AttackMotionProfile {
            windup_scale: 0.92,
            force_scale: 0.94,
            precision_scale: 0.72,
        },
    }
}

pub fn select_attack_motion(
    skill: f32,
    tick: u64,
    attacker: EntityKey,
    defender: EntityKey,
    height_diff: f32,
) -> AttackMotion {
    if height_diff > 1.0 {
        return AttackMotion::Overhead;
    }

    let skill = skill.clamp(0.0, 1.0);
    let roll = pair_hash_unit(tick, attacker, defender, 11);
    if skill < 0.25 {
        if roll < 0.65 {
            AttackMotion::Forehand
        } else {
            AttackMotion::Overhead
        }
    } else if skill < 0.65 {
        if roll < 0.4 {
            AttackMotion::Forehand
        } else if roll < 0.7 {
            AttackMotion::Backhand
        } else {
            AttackMotion::Thrust
        }
    } else if roll < 0.25 {
        AttackMotion::Forehand
    } else if roll < 0.5 {
        AttackMotion::Backhand
    } else if roll < 0.75 {
        AttackMotion::Thrust
    } else {
        AttackMotion::Overhead
    }
}

pub fn optimal_block_for(motion: AttackMotion, height_diff: f32) -> BlockManeuver {
    match motion {
        AttackMotion::Generic => BlockManeuver::Generic,
        AttackMotion::Overhead => {
            if height_diff > 0.5 {
                BlockManeuver::HighGuard
            } else {
                BlockManeuver::OutsideParry
            }
        }
        AttackMotion::Forehand => BlockManeuver::OutsideParry,
        AttackMotion::Backhand => BlockManeuver::InsideParry,
        AttackMotion::Thrust => BlockManeuver::LowGuard,
    }
}

pub fn select_block_maneuver(
    skill: f32,
    motion: AttackMotion,
    height_diff: f32,
    tick: u64,
    defender: EntityKey,
    attacker: EntityKey,
) -> BlockManeuver {
    let skill = skill.clamp(0.0, 1.0);
    let best = optimal_block_for(motion, height_diff);
    let roll = pair_hash_unit(tick, defender, attacker, 23);
    let read_threshold = 0.15 + skill * 0.8;
    if roll <= read_threshold {
        return best;
    }

    match best {
        BlockManeuver::HighGuard => {
            if roll < 0.75 {
                BlockManeuver::OutsideParry
            } else {
                BlockManeuver::LowGuard
            }
        }
        BlockManeuver::InsideParry => {
            if roll < 0.75 {
                BlockManeuver::OutsideParry
            } else {
                BlockManeuver::HighGuard
            }
        }
        BlockManeuver::OutsideParry => {
            if roll < 0.75 {
                BlockManeuver::InsideParry
            } else {
                BlockManeuver::HighGuard
            }
        }
        BlockManeuver::LowGuard => {
            if roll < 0.75 {
                BlockManeuver::InsideParry
            } else {
                BlockManeuver::OutsideParry
            }
        }
        BlockManeuver::Generic => BlockManeuver::Generic,
    }
}

pub fn block_effectiveness(maneuver: BlockManeuver, motion: AttackMotion, height_diff: f32) -> f32 {
    let best = optimal_block_for(motion, height_diff);
    let mut effectiveness: f32 = if maneuver == best {
        1.0
    } else {
        match (maneuver, best) {
            (BlockManeuver::InsideParry, BlockManeuver::OutsideParry)
            | (BlockManeuver::OutsideParry, BlockManeuver::InsideParry)
            | (BlockManeuver::HighGuard, BlockManeuver::OutsideParry)
            | (BlockManeuver::OutsideParry, BlockManeuver::HighGuard)
            | (BlockManeuver::LowGuard, BlockManeuver::InsideParry)
            | (BlockManeuver::InsideParry, BlockManeuver::LowGuard) => 0.72,
            (BlockManeuver::Generic, _) => 0.55,
            _ => 0.35,
        }
    };

    if motion == AttackMotion::Overhead && height_diff > 0.5 && maneuver != BlockManeuver::HighGuard
    {
        effectiveness *= 0.75;
    }
    effectiveness.clamp(0.1, 1.0)
}

fn pair_hash_unit(tick: u64, a: EntityKey, b: EntityKey, salt: u32) -> f32 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tick.hash(&mut hasher);
    let a_bits: u64 = unsafe { std::mem::transmute(a) };
    let b_bits: u64 = unsafe { std::mem::transmute(b) };
    a_bits.hash(&mut hasher);
    b_bits.hash(&mut hasher);
    salt.hash(&mut hasher);
    let h = hasher.finish();
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn keys() -> (EntityKey, EntityKey) {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        (sm.insert(()), sm.insert(()))
    }

    #[test]
    fn high_ground_prefers_overhead() {
        let (a, d) = keys();
        assert_eq!(
            select_attack_motion(0.8, 0, a, d, 2.0),
            AttackMotion::Overhead
        );
    }

    #[test]
    fn skilled_defender_reads_best_block() {
        let (a, d) = keys();
        let maneuver = select_block_maneuver(1.0, AttackMotion::Thrust, 0.0, 10, d, a);
        assert_eq!(maneuver, BlockManeuver::LowGuard);
    }

    #[test]
    fn overhead_on_lower_defender_penalizes_non_high_guard() {
        let high = block_effectiveness(BlockManeuver::HighGuard, AttackMotion::Overhead, 2.0);
        let low = block_effectiveness(BlockManeuver::OutsideParry, AttackMotion::Overhead, 2.0);
        assert!(high > low);
    }
}
