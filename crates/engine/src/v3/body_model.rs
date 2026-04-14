use serde::{Deserialize, Serialize};

use super::spatial::Vec3;

// ---------------------------------------------------------------------------
// Body point identification
// ---------------------------------------------------------------------------

/// The 16 skeletal points of the body model. Index order matches array layout
/// in `BodyModel::points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BodyPointId {
    Head = 0,
    Neck = 1,
    LeftShoulder = 2,
    RightShoulder = 3,
    LeftElbow = 4,
    RightElbow = 5,
    LeftHand = 6,
    RightHand = 7,
    UpperSpine = 8,
    LowerSpine = 9,
    LeftHip = 10,
    RightHip = 11,
    LeftKnee = 12,
    RightKnee = 13,
    LeftFoot = 14,
    RightFoot = 15,
}

impl BodyPointId {
    pub const COUNT: usize = 16;

    pub const ALL: [BodyPointId; 16] = [
        Self::Head,
        Self::Neck,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::LeftElbow,
        Self::RightElbow,
        Self::LeftHand,
        Self::RightHand,
        Self::UpperSpine,
        Self::LowerSpine,
        Self::LeftHip,
        Self::RightHip,
        Self::LeftKnee,
        Self::RightKnee,
        Self::LeftFoot,
        Self::RightFoot,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

// ---------------------------------------------------------------------------
// Body point
// ---------------------------------------------------------------------------

/// A single point mass in the Verlet body model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BodyPoint {
    /// Current position (absolute world coordinates).
    pub pos: Vec3,
    /// Previous position (velocity = pos - prev_pos).
    pub prev_pos: Vec3,
    /// Mass in kg, for force response and inertia.
    pub mass: f32,
}

impl BodyPoint {
    pub fn new(pos: Vec3, mass: f32) -> Self {
        Self {
            pos,
            prev_pos: pos,
            mass,
        }
    }

    /// Implicit velocity from Verlet integration.
    pub fn velocity(&self) -> Vec3 {
        self.pos - self.prev_pos
    }
}

// ---------------------------------------------------------------------------
// Constraints
// ---------------------------------------------------------------------------

/// Distance constraint between two body points.
#[derive(Debug, Clone, Copy)]
pub struct DistanceConstraint {
    pub a: BodyPointId,
    pub b: BodyPointId,
    pub rest_length: f32,
}

/// Angular constraint at a pivot joint.
#[derive(Debug, Clone, Copy)]
pub struct AngularConstraint {
    pub a: BodyPointId,
    pub pivot: BodyPointId,
    pub b: BodyPointId,
    /// Minimum angle in radians.
    pub min_angle: f32,
    /// Maximum angle in radians.
    pub max_angle: f32,
}

// ---------------------------------------------------------------------------
// Default skeleton topology
// ---------------------------------------------------------------------------

/// 17 distance constraints defining the skeletal segments.
pub const SKELETON_DISTANCES: [DistanceConstraint; 17] = [
    // Neck
    DistanceConstraint {
        a: BodyPointId::Head,
        b: BodyPointId::Neck,
        rest_length: 0.15,
    },
    // Left upper arm
    DistanceConstraint {
        a: BodyPointId::LeftShoulder,
        b: BodyPointId::LeftElbow,
        rest_length: 0.30,
    },
    // Right upper arm
    DistanceConstraint {
        a: BodyPointId::RightShoulder,
        b: BodyPointId::RightElbow,
        rest_length: 0.30,
    },
    // Left forearm
    DistanceConstraint {
        a: BodyPointId::LeftElbow,
        b: BodyPointId::LeftHand,
        rest_length: 0.28,
    },
    // Right forearm
    DistanceConstraint {
        a: BodyPointId::RightElbow,
        b: BodyPointId::RightHand,
        rest_length: 0.28,
    },
    // Upper torso
    DistanceConstraint {
        a: BodyPointId::Neck,
        b: BodyPointId::UpperSpine,
        rest_length: 0.25,
    },
    // Lower torso
    DistanceConstraint {
        a: BodyPointId::UpperSpine,
        b: BodyPointId::LowerSpine,
        rest_length: 0.25,
    },
    // Left shoulder mount
    DistanceConstraint {
        a: BodyPointId::Neck,
        b: BodyPointId::LeftShoulder,
        rest_length: 0.20,
    },
    // Right shoulder mount
    DistanceConstraint {
        a: BodyPointId::Neck,
        b: BodyPointId::RightShoulder,
        rest_length: 0.20,
    },
    // Left thigh
    DistanceConstraint {
        a: BodyPointId::LeftHip,
        b: BodyPointId::LeftKnee,
        rest_length: 0.45,
    },
    // Right thigh
    DistanceConstraint {
        a: BodyPointId::RightHip,
        b: BodyPointId::RightKnee,
        rest_length: 0.45,
    },
    // Left shin
    DistanceConstraint {
        a: BodyPointId::LeftKnee,
        b: BodyPointId::LeftFoot,
        rest_length: 0.42,
    },
    // Right shin
    DistanceConstraint {
        a: BodyPointId::RightKnee,
        b: BodyPointId::RightFoot,
        rest_length: 0.42,
    },
    // Left hip mount
    DistanceConstraint {
        a: BodyPointId::LowerSpine,
        b: BodyPointId::LeftHip,
        rest_length: 0.15,
    },
    // Right hip mount
    DistanceConstraint {
        a: BodyPointId::LowerSpine,
        b: BodyPointId::RightHip,
        rest_length: 0.15,
    },
    // Weapon (placeholder rest_length — overridden by actual weapon reach)
    DistanceConstraint {
        a: BodyPointId::RightHand,
        b: BodyPointId::Head, // Placeholder — sword tip is an equipment point, not in the 16
        rest_length: 1.0,
    },
    // Shield mount (placeholder)
    DistanceConstraint {
        a: BodyPointId::LeftHand,
        b: BodyPointId::Head, // Placeholder — shield disc is equipment
        rest_length: 0.15,
    },
];

/// The 15 core distance constraints (excludes weapon/shield equipment).
pub const CORE_DISTANCES: &[DistanceConstraint] = {
    // Can't slice const arrays at compile time, so we define inline
    &[
        DistanceConstraint {
            a: BodyPointId::Head,
            b: BodyPointId::Neck,
            rest_length: 0.15,
        },
        DistanceConstraint {
            a: BodyPointId::LeftShoulder,
            b: BodyPointId::LeftElbow,
            rest_length: 0.30,
        },
        DistanceConstraint {
            a: BodyPointId::RightShoulder,
            b: BodyPointId::RightElbow,
            rest_length: 0.30,
        },
        DistanceConstraint {
            a: BodyPointId::LeftElbow,
            b: BodyPointId::LeftHand,
            rest_length: 0.28,
        },
        DistanceConstraint {
            a: BodyPointId::RightElbow,
            b: BodyPointId::RightHand,
            rest_length: 0.28,
        },
        DistanceConstraint {
            a: BodyPointId::Neck,
            b: BodyPointId::UpperSpine,
            rest_length: 0.25,
        },
        DistanceConstraint {
            a: BodyPointId::UpperSpine,
            b: BodyPointId::LowerSpine,
            rest_length: 0.25,
        },
        DistanceConstraint {
            a: BodyPointId::Neck,
            b: BodyPointId::LeftShoulder,
            rest_length: 0.20,
        },
        DistanceConstraint {
            a: BodyPointId::Neck,
            b: BodyPointId::RightShoulder,
            rest_length: 0.20,
        },
        DistanceConstraint {
            a: BodyPointId::LeftHip,
            b: BodyPointId::LeftKnee,
            rest_length: 0.45,
        },
        DistanceConstraint {
            a: BodyPointId::RightHip,
            b: BodyPointId::RightKnee,
            rest_length: 0.45,
        },
        DistanceConstraint {
            a: BodyPointId::LeftKnee,
            b: BodyPointId::LeftFoot,
            rest_length: 0.42,
        },
        DistanceConstraint {
            a: BodyPointId::RightKnee,
            b: BodyPointId::RightFoot,
            rest_length: 0.42,
        },
        DistanceConstraint {
            a: BodyPointId::LowerSpine,
            b: BodyPointId::LeftHip,
            rest_length: 0.15,
        },
        DistanceConstraint {
            a: BodyPointId::LowerSpine,
            b: BodyPointId::RightHip,
            rest_length: 0.15,
        },
    ]
};

/// 5 angular constraints at major joints.
pub const SKELETON_ANGLES: [AngularConstraint; 5] = [
    // Left elbow: 15-170 degrees
    AngularConstraint {
        a: BodyPointId::LeftShoulder,
        pivot: BodyPointId::LeftElbow,
        b: BodyPointId::LeftHand,
        min_angle: 0.2618, // ~15 deg
        max_angle: 2.9671, // ~170 deg
    },
    // Right elbow: 15-170 degrees
    AngularConstraint {
        a: BodyPointId::RightShoulder,
        pivot: BodyPointId::RightElbow,
        b: BodyPointId::RightHand,
        min_angle: 0.2618,
        max_angle: 2.9671,
    },
    // Left knee: 10-165 degrees
    AngularConstraint {
        a: BodyPointId::LeftHip,
        pivot: BodyPointId::LeftKnee,
        b: BodyPointId::LeftFoot,
        min_angle: 0.1745, // ~10 deg
        max_angle: 2.8798, // ~165 deg
    },
    // Right knee: 10-165 degrees
    AngularConstraint {
        a: BodyPointId::RightHip,
        pivot: BodyPointId::RightKnee,
        b: BodyPointId::RightFoot,
        min_angle: 0.1745,
        max_angle: 2.8798,
    },
    // Left shoulder: 0-180 degrees
    AngularConstraint {
        a: BodyPointId::Neck,
        pivot: BodyPointId::LeftShoulder,
        b: BodyPointId::LeftElbow,
        min_angle: 0.0,
        max_angle: std::f32::consts::PI,
    },
];

// ---------------------------------------------------------------------------
// Default point masses (kg)
// ---------------------------------------------------------------------------

/// Default mass for each body point.
pub const DEFAULT_MASSES: [f32; BodyPointId::COUNT] = [
    5.0,  // Head
    2.0,  // Neck
    3.0,  // Left shoulder
    3.0,  // Right shoulder
    2.0,  // Left elbow
    2.0,  // Right elbow
    1.0,  // Left hand
    1.0,  // Right hand
    8.0,  // Upper spine
    10.0, // Lower spine
    5.0,  // Left hip
    5.0,  // Right hip
    3.0,  // Left knee
    3.0,  // Right knee
    2.0,  // Left foot
    2.0,  // Right foot
];

// ---------------------------------------------------------------------------
// Stance system
// ---------------------------------------------------------------------------

/// Compact stance identifier for idle entities (no full body model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum StanceId {
    Neutral = 0,
    HighGuard = 1,
    MidGuard = 2,
    LowGuard = 3,
    Lunge = 4,
    Withdraw = 5,
    // Civilian stances
    ToolSwing = 6,
    Carry = 7,
    Climb = 8,
}

/// Target offsets for each body point in a stance, relative to entity root
/// in body-local coordinates (x = right, y = forward/facing, z = up).
#[derive(Debug, Clone)]
pub struct StanceTemplate {
    pub id: StanceId,
    /// Target offset from entity root for each of the 16 points.
    pub offsets: [Vec3; BodyPointId::COUNT],
    /// How strongly each point is pulled toward its target (0.0 = free, 1.0 = rigid).
    pub stiffness: f32,
}

/// Standing height from feet to head in neutral stance (~1.75m).
const STANDING_HEIGHT: f32 = 1.75;
const FOOT_Z: f32 = 0.0;
const KNEE_Z: f32 = 0.42;
const HIP_Z: f32 = 0.87;
const LOWER_SPINE_Z: f32 = 0.87;
const UPPER_SPINE_Z: f32 = 1.12;
const NECK_Z: f32 = 1.37;
const SHOULDER_Z: f32 = 1.37;
const HEAD_Z: f32 = STANDING_HEIGHT - 0.23;
const ELBOW_Z: f32 = 1.07;
const HAND_Z: f32 = 0.79;

const HIP_HALF_WIDTH: f32 = 0.15;
const SHOULDER_HALF_WIDTH: f32 = 0.20;

pub fn neutral_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::Neutral,
        offsets: [
            Vec3::new(0.0, 0.0, HEAD_Z),                      // Head
            Vec3::new(0.0, 0.0, NECK_Z),                      // Neck
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z), // L shoulder
            Vec3::new(SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),  // R shoulder
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, ELBOW_Z),    // L elbow
            Vec3::new(SHOULDER_HALF_WIDTH, 0.0, ELBOW_Z),     // R elbow
            Vec3::new(-0.15, 0.0, HAND_Z),                    // L hand
            Vec3::new(0.15, 0.0, HAND_Z),                     // R hand
            Vec3::new(0.0, 0.0, UPPER_SPINE_Z),               // Upper spine
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),               // Lower spine
            Vec3::new(-HIP_HALF_WIDTH, 0.0, HIP_Z),           // L hip
            Vec3::new(HIP_HALF_WIDTH, 0.0, HIP_Z),            // R hip
            Vec3::new(-HIP_HALF_WIDTH, 0.0, KNEE_Z),          // L knee
            Vec3::new(HIP_HALF_WIDTH, 0.0, KNEE_Z),           // R knee
            Vec3::new(-HIP_HALF_WIDTH, 0.0, FOOT_Z),          // L foot
            Vec3::new(HIP_HALF_WIDTH, 0.0, FOOT_Z),           // R foot
        ],
        stiffness: 0.3,
    }
}

pub fn high_guard_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::HighGuard,
        offsets: [
            Vec3::new(0.0, 0.0, HEAD_Z),
            Vec3::new(0.0, 0.0, NECK_Z),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(-0.10, 0.10, 1.30), // L elbow raised
            Vec3::new(0.05, 0.10, 1.55),  // R elbow high
            Vec3::new(-0.10, 0.15, 1.20), // L hand guard
            Vec3::new(0.0, 0.05, STANDING_HEIGHT + 0.05), // R hand above head
            Vec3::new(0.0, 0.0, UPPER_SPINE_Z),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, KNEE_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, KNEE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, FOOT_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, FOOT_Z),
        ],
        stiffness: 0.6,
    }
}

pub fn mid_guard_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::MidGuard,
        offsets: [
            Vec3::new(0.0, 0.0, HEAD_Z),
            Vec3::new(0.0, 0.0, NECK_Z),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(-0.15, 0.10, 1.15), // L elbow forward
            Vec3::new(0.15, 0.15, 1.15),  // R elbow forward
            Vec3::new(-0.10, 0.20, 1.10), // L hand guard
            Vec3::new(0.10, 0.25, 1.20),  // R hand center chest
            Vec3::new(0.0, 0.0, UPPER_SPINE_Z),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, KNEE_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.05, KNEE_Z), // Lead foot slightly forward
            Vec3::new(-HIP_HALF_WIDTH, 0.0, FOOT_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.10, FOOT_Z), // R foot forward
        ],
        stiffness: 0.6,
    }
}

pub fn low_guard_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::LowGuard,
        offsets: [
            Vec3::new(0.0, 0.0, HEAD_Z),
            Vec3::new(0.0, 0.0, NECK_Z),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(-0.15, 0.05, 1.00), // L elbow low
            Vec3::new(0.20, 0.05, 0.90),  // R elbow low
            Vec3::new(-0.10, 0.10, 0.85), // L hand low
            Vec3::new(0.20, 0.10, 0.70),  // R hand below waist
            Vec3::new(0.0, 0.0, UPPER_SPINE_Z),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-0.20, 0.0, HIP_Z), // Wide stance
            Vec3::new(0.20, 0.0, HIP_Z),
            Vec3::new(-0.20, 0.0, KNEE_Z),
            Vec3::new(0.20, 0.0, KNEE_Z),
            Vec3::new(-0.20, 0.0, FOOT_Z),
            Vec3::new(0.20, 0.0, FOOT_Z),
        ],
        stiffness: 0.5,
    }
}

pub fn lunge_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::Lunge,
        offsets: [
            Vec3::new(0.0, 0.15, HEAD_Z - 0.10), // Head forward, slightly lower
            Vec3::new(0.0, 0.10, NECK_Z - 0.08),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.05, SHOULDER_Z - 0.05),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.10, SHOULDER_Z - 0.05),
            Vec3::new(-0.15, 0.10, 1.05),
            Vec3::new(0.10, 0.35, 1.15), // R elbow extended forward
            Vec3::new(-0.10, 0.15, 0.95),
            Vec3::new(0.05, 0.50, 1.20), // R hand max extension
            Vec3::new(0.0, 0.08, UPPER_SPINE_Z - 0.03),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-HIP_HALF_WIDTH, -0.10, HIP_Z), // Back hip
            Vec3::new(HIP_HALF_WIDTH, 0.10, HIP_Z - 0.05), // Lead hip forward+lower
            Vec3::new(-HIP_HALF_WIDTH, -0.15, KNEE_Z + 0.05),
            Vec3::new(HIP_HALF_WIDTH, 0.25, KNEE_Z - 0.10), // Deep lead knee
            Vec3::new(-HIP_HALF_WIDTH, -0.20, FOOT_Z),      // Back foot behind
            Vec3::new(HIP_HALF_WIDTH, 0.40, FOOT_Z),        // Lead foot far forward
        ],
        stiffness: 0.7,
    }
}

pub fn withdraw_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::Withdraw,
        offsets: [
            Vec3::new(0.0, -0.05, HEAD_Z),
            Vec3::new(0.0, -0.03, NECK_Z),
            Vec3::new(-SHOULDER_HALF_WIDTH, -0.03, SHOULDER_Z),
            Vec3::new(SHOULDER_HALF_WIDTH, -0.03, SHOULDER_Z),
            Vec3::new(-0.12, -0.05, 1.15),
            Vec3::new(0.12, -0.05, 1.20), // R elbow retracted
            Vec3::new(-0.08, 0.00, 1.10),
            Vec3::new(0.08, 0.00, 1.25), // R hand to chest
            Vec3::new(0.0, -0.02, UPPER_SPINE_Z),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-0.10, 0.0, HIP_Z), // Narrow stance
            Vec3::new(0.10, 0.0, HIP_Z),
            Vec3::new(-0.10, 0.0, KNEE_Z),
            Vec3::new(0.10, -0.05, KNEE_Z), // Weight back
            Vec3::new(-0.10, 0.0, FOOT_Z),
            Vec3::new(0.10, -0.10, FOOT_Z),
        ],
        stiffness: 0.5,
    }
}

// ---------------------------------------------------------------------------
// Civilian stances
// ---------------------------------------------------------------------------

pub fn tool_swing_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::ToolSwing,
        offsets: [
            Vec3::new(0.0, 0.05, HEAD_Z),
            Vec3::new(0.0, 0.03, NECK_Z),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.05, SHOULDER_Z),
            Vec3::new(-0.15, 0.05, 1.10),           // L elbow forward
            Vec3::new(0.10, 0.20, 1.30),            // R elbow high/forward
            Vec3::new(-0.10, 0.15, 1.00),           // L hand on tool shaft
            Vec3::new(0.05, 0.10, STANDING_HEIGHT), // R hand high (tool grip)
            Vec3::new(0.0, 0.02, UPPER_SPINE_Z),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-0.18, 0.0, HIP_Z), // Wide stance for stability
            Vec3::new(0.18, 0.0, HIP_Z),
            Vec3::new(-0.18, 0.0, KNEE_Z),
            Vec3::new(0.18, 0.0, KNEE_Z),
            Vec3::new(-0.18, 0.0, FOOT_Z),
            Vec3::new(0.18, 0.0, FOOT_Z),
        ],
        stiffness: 0.5,
    }
}

pub fn carry_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::Carry,
        offsets: [
            Vec3::new(0.0, -0.02, HEAD_Z - 0.02), // Slightly hunched
            Vec3::new(0.0, -0.01, NECK_Z - 0.01),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z - 0.02),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.0, SHOULDER_Z - 0.02),
            Vec3::new(-0.20, 0.10, 1.00), // Arms forward, holding
            Vec3::new(0.20, 0.10, 1.00),
            Vec3::new(-0.15, 0.20, 0.95), // Hands gripping load
            Vec3::new(0.15, 0.20, 0.95),
            Vec3::new(0.0, -0.01, UPPER_SPINE_Z - 0.02),
            Vec3::new(0.0, 0.0, LOWER_SPINE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, KNEE_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, KNEE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, FOOT_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, FOOT_Z),
        ],
        stiffness: 0.7, // Stiff — arms locked around load
    }
}

pub fn climb_stance() -> StanceTemplate {
    StanceTemplate {
        id: StanceId::Climb,
        offsets: [
            Vec3::new(0.0, 0.15, HEAD_Z + 0.10), // Head forward and up
            Vec3::new(0.0, 0.10, NECK_Z + 0.05),
            Vec3::new(-SHOULDER_HALF_WIDTH, 0.08, SHOULDER_Z + 0.05),
            Vec3::new(SHOULDER_HALF_WIDTH, 0.08, SHOULDER_Z + 0.05),
            Vec3::new(-0.15, 0.15, 1.40), // L elbow reaching up
            Vec3::new(0.15, 0.15, 1.20),  // R elbow lower (alternating grip)
            Vec3::new(-0.10, 0.20, 1.55), // L hand gripping above
            Vec3::new(0.10, 0.20, 1.30),  // R hand gripping lower
            Vec3::new(0.0, 0.10, UPPER_SPINE_Z + 0.03),
            Vec3::new(0.0, 0.05, LOWER_SPINE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(HIP_HALF_WIDTH, 0.0, HIP_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.05, KNEE_Z + 0.10), // L knee raised (stepping)
            Vec3::new(HIP_HALF_WIDTH, 0.0, KNEE_Z),
            Vec3::new(-HIP_HALF_WIDTH, 0.10, FOOT_Z + 0.20), // L foot on hold
            Vec3::new(HIP_HALF_WIDTH, 0.0, FOOT_Z),          // R foot planted
        ],
        stiffness: 0.6,
    }
}

/// Get the stance template for a given ID.
pub fn stance_template(id: StanceId) -> StanceTemplate {
    match id {
        StanceId::Neutral => neutral_stance(),
        StanceId::HighGuard => high_guard_stance(),
        StanceId::MidGuard => mid_guard_stance(),
        StanceId::LowGuard => low_guard_stance(),
        StanceId::Lunge => lunge_stance(),
        StanceId::Withdraw => withdraw_stance(),
        StanceId::ToolSwing => tool_swing_stance(),
        StanceId::Carry => carry_stance(),
        StanceId::Climb => climb_stance(),
    }
}

// ---------------------------------------------------------------------------
// Body model
// ---------------------------------------------------------------------------

/// The 16-point 3D Verlet skeletal body model. Activated when an entity is
/// performing a physical action at sufficient tick resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyModel {
    /// 16 body points in world coordinates.
    pub points: [BodyPoint; BodyPointId::COUNT],
    /// Current stance (drives spring forces toward target positions).
    pub stance: StanceId,
    /// External forces accumulated this tick, applied during substep.
    /// Cleared after physics step.
    #[serde(skip)]
    pub external_forces: [Vec3; BodyPointId::COUNT],
}

impl BodyModel {
    /// Construct a body model at the given root position and facing, using the
    /// specified stance template. Points are placed at world positions by
    /// rotating body-local offsets by the facing angle and translating to root.
    pub fn from_stance(root: Vec3, facing: f32, stance_id: StanceId) -> Self {
        let template = stance_template(stance_id);
        let cos_f = facing.cos();
        let sin_f = facing.sin();
        let mut points = [BodyPoint::new(Vec3::ZERO, 1.0); BodyPointId::COUNT];

        for (i, offset) in template.offsets.iter().enumerate() {
            // Rotate body-local (x=right, y=forward) into world XY plane
            let world_x = offset.x * cos_f - offset.y * sin_f;
            let world_y = offset.x * sin_f + offset.y * cos_f;
            let world_pos = Vec3::new(root.x + world_x, root.y + world_y, root.z + offset.z);
            points[i] = BodyPoint::new(world_pos, DEFAULT_MASSES[i]);
        }

        Self {
            points,
            stance: stance_id,
            external_forces: [Vec3::ZERO; BodyPointId::COUNT],
        }
    }

    /// Get a body point by ID.
    pub fn point(&self, id: BodyPointId) -> &BodyPoint {
        &self.points[id.index()]
    }

    /// Get a mutable body point by ID.
    pub fn point_mut(&mut self, id: BodyPointId) -> &mut BodyPoint {
        &mut self.points[id.index()]
    }

    /// Apply an external force to a specific body point (accumulated, applied
    /// during next physics substep).
    pub fn apply_force(&mut self, id: BodyPointId, force: Vec3) {
        let f = &mut self.external_forces[id.index()];
        *f = *f + force;
    }

    /// Clear accumulated external forces after a physics step.
    pub fn clear_forces(&mut self) {
        self.external_forces = [Vec3::ZERO; BodyPointId::COUNT];
    }

    /// Transition to a new stance. This just sets the target — the physics
    /// solver applies spring forces toward the new target positions over
    /// subsequent ticks.
    pub fn set_stance(&mut self, stance_id: StanceId) {
        self.stance = stance_id;
    }

    /// Approximate root position (lower spine).
    pub fn root_pos(&self) -> Vec3 {
        self.points[BodyPointId::LowerSpine.index()].pos
    }

    /// Total kinetic energy (for stability monitoring).
    pub fn kinetic_energy(&self) -> f32 {
        self.points
            .iter()
            .map(|p| 0.5 * p.mass * p.velocity().length_squared())
            .sum()
    }

    // -- Footwork measurements --

    /// Stance width: horizontal distance between feet (2D, ignoring z).
    pub fn stance_width(&self) -> f32 {
        let lf = self.point(BodyPointId::LeftFoot).pos;
        let rf = self.point(BodyPointId::RightFoot).pos;
        let dx = rf.x - lf.x;
        let dy = rf.y - lf.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Forward reach: distance from lower spine to right hand in the XY plane.
    /// Represents how far the weapon hand extends forward.
    pub fn forward_reach(&self) -> f32 {
        let spine = self.point(BodyPointId::LowerSpine).pos;
        let hand = self.point(BodyPointId::RightHand).pos;
        let dx = hand.x - spine.x;
        let dy = hand.y - spine.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Lunge reach: distance from rear foot to right hand.
    /// Represents total reach including body extension.
    pub fn lunge_reach(&self) -> f32 {
        let lf = self.point(BodyPointId::LeftFoot).pos; // rear foot in lunge
        let hand = self.point(BodyPointId::RightHand).pos;
        let dx = hand.x - lf.x;
        let dy = hand.y - lf.y;
        (dx * dx + dy * dy).sqrt()
    }
}

// ---------------------------------------------------------------------------
// Wound effects on body mechanics
// ---------------------------------------------------------------------------

/// Compute stance width reduction from leg wound weight.
/// Returns a multiplier (0.0–1.0) for how much the constraint solver
/// should narrow the stance.
///
/// `leg_wound_weight`: sum of severity weights for leg wounds (0.0 = healthy).
pub fn wound_stance_factor(leg_wound_weight: f32) -> f32 {
    (1.0 - leg_wound_weight * 0.5).clamp(0.3, 1.0)
}

/// Compute kinetic chain force reduction from arm wound weight.
/// Returns a multiplier (0.0–1.0) for chain link force magnitude.
///
/// `arm_wound_weight`: sum of severity weights for arm wounds.
pub fn wound_arm_force_factor(arm_wound_weight: f32) -> f32 {
    (1.0 - arm_wound_weight * 0.6).clamp(0.2, 1.0)
}

/// Compute spine twist range reduction from torso wound weight.
/// Returns a multiplier (0.0–1.0) for angular constraint range.
pub fn wound_torso_twist_factor(torso_wound_weight: f32) -> f32 {
    (1.0 - torso_wound_weight * 0.4).clamp(0.3, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_point_id_indices() {
        assert_eq!(BodyPointId::Head.index(), 0);
        assert_eq!(BodyPointId::RightFoot.index(), 15);
        assert_eq!(BodyPointId::ALL.len(), BodyPointId::COUNT);
    }

    #[test]
    fn body_model_from_neutral_stance() {
        let bm = BodyModel::from_stance(Vec3::new(10.0, 20.0, 0.0), 0.0, StanceId::Neutral);

        // Head should be above root
        let head = bm.point(BodyPointId::Head);
        assert!(head.pos.z > 1.0, "head z={}", head.pos.z);

        // Feet should be at ground level (root z)
        let lf = bm.point(BodyPointId::LeftFoot);
        assert!((lf.pos.z - 0.0).abs() < 0.01, "left foot z={}", lf.pos.z);

        // All points initialized with zero velocity
        for p in &bm.points {
            assert_eq!(p.velocity().length(), 0.0);
        }
    }

    #[test]
    fn body_model_facing_rotates_points() {
        let root = Vec3::new(0.0, 0.0, 0.0);
        let bm0 = BodyModel::from_stance(root, 0.0, StanceId::Neutral);
        let bm90 = BodyModel::from_stance(root, std::f32::consts::FRAC_PI_2, StanceId::Neutral);

        // Left shoulder at facing=0 should be at negative x
        let ls0 = bm0.point(BodyPointId::LeftShoulder);
        assert!(ls0.pos.x < 0.0, "ls0.x={}", ls0.pos.x);

        // Left shoulder at facing=PI/2 should be at positive-ish y
        // (rotated 90 degrees)
        let ls90 = bm90.point(BodyPointId::LeftShoulder);
        assert!(
            (ls90.pos.z - ls0.pos.z).abs() < 0.01,
            "z should be the same"
        );
    }

    #[test]
    fn stance_templates_have_correct_id() {
        assert_eq!(neutral_stance().id, StanceId::Neutral);
        assert_eq!(high_guard_stance().id, StanceId::HighGuard);
        assert_eq!(lunge_stance().id, StanceId::Lunge);
    }

    #[test]
    fn kinetic_energy_zero_at_rest() {
        let bm = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        assert_eq!(bm.kinetic_energy(), 0.0);
    }

    #[test]
    fn external_force_accumulates() {
        let mut bm = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        bm.apply_force(BodyPointId::Head, Vec3::new(1.0, 0.0, 0.0));
        bm.apply_force(BodyPointId::Head, Vec3::new(0.0, 2.0, 0.0));
        let f = bm.external_forces[BodyPointId::Head.index()];
        assert!((f.x - 1.0).abs() < 1e-6);
        assert!((f.y - 2.0).abs() < 1e-6);
        bm.clear_forces();
        let f2 = bm.external_forces[BodyPointId::Head.index()];
        assert_eq!(f2.length(), 0.0);
    }

    // --- Footwork tests ---

    #[test]
    fn lunge_increases_reach() {
        let neutral = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        let lunge = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Lunge);

        let neutral_reach = neutral.lunge_reach();
        let lunge_reach = lunge.lunge_reach();

        assert!(
            lunge_reach > neutral_reach + 0.2,
            "lunge should increase reach by ~0.3m: neutral={neutral_reach:.2}, lunge={lunge_reach:.2}"
        );
    }

    #[test]
    fn lunge_stance_wider_than_withdraw() {
        let lunge = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Lunge);
        let withdraw = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Withdraw);

        assert!(
            lunge.stance_width() > withdraw.stance_width(),
            "lunge should be wider: lunge={:.2}, withdraw={:.2}",
            lunge.stance_width(),
            withdraw.stance_width()
        );
    }

    #[test]
    fn wound_stance_factor_healthy() {
        assert!((wound_stance_factor(0.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn wound_stance_factor_laceration() {
        let f = wound_stance_factor(0.4);
        assert!(f < 1.0 && f > 0.5, "laceration should narrow stance: {f}");
    }

    #[test]
    fn wound_arm_force_reduces_swing() {
        let healthy = wound_arm_force_factor(0.0);
        let wounded = wound_arm_force_factor(0.6);
        assert!(
            wounded < healthy,
            "arm wound should reduce force: healthy={healthy}, wounded={wounded}"
        );
    }

    #[test]
    fn civilian_stances_exist() {
        assert_eq!(tool_swing_stance().id, StanceId::ToolSwing);
        assert_eq!(carry_stance().id, StanceId::Carry);
        assert_eq!(climb_stance().id, StanceId::Climb);
    }
}
