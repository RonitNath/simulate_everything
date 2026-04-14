use serde::{Deserialize, Serialize};

use super::armor::MaterialType;
use simulate_everything_protocol::{CommodityKind, MaterialKind, MatterState, PropertyTag};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalProperties {
    pub mass_kg: f32,
    pub hardness: f32,
    pub temperature_k: f32,
    pub material: MaterialKind,
    pub matter_state: MatterState,
    pub tags: u64,
}

impl PhysicalProperties {
    pub const AMBIENT_TEMPERATURE_K: f32 = 293.0;

    pub fn new(
        mass_kg: f32,
        hardness: f32,
        material: MaterialKind,
        matter_state: MatterState,
    ) -> Self {
        Self {
            mass_kg,
            hardness,
            temperature_k: Self::AMBIENT_TEMPERATURE_K,
            material,
            matter_state,
            tags: 0,
        }
    }

    pub fn with_tags(mut self, tags: &[PropertyTag]) -> Self {
        for tag in tags {
            self.insert_tag(*tag);
        }
        self
    }

    pub fn insert_tag(&mut self, tag: PropertyTag) {
        self.tags |= tag_mask(tag);
    }

    pub fn has_tag(&self, tag: PropertyTag) -> bool {
        self.tags & tag_mask(tag) != 0
    }

    pub fn has_all_tags(&self, tags: &[PropertyTag]) -> bool {
        tags.iter().all(|tag| self.has_tag(*tag))
    }

    pub fn tags_vec(&self) -> Vec<PropertyTag> {
        ALL_PROPERTY_TAGS
            .iter()
            .copied()
            .filter(|tag| self.has_tag(*tag))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProperties {
    pub force_mult: f32,
    pub precision: f32,
    pub cutting_edge: f32,
    pub heat_output_k: f32,
    pub capacity_l: f32,
    pub durability: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatterStack {
    pub commodity: CommodityKind,
    pub amount: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteProperties {
    pub build_progress: f32,
    pub integrity: f32,
    pub occupancy_capacity: usize,
}

pub const ALL_PROPERTY_TAGS: [PropertyTag; 13] = [
    PropertyTag::Harvestable,
    PropertyTag::Edible,
    PropertyTag::Fuel,
    PropertyTag::HeatSource,
    PropertyTag::Tool,
    PropertyTag::Container,
    PropertyTag::Shelter,
    PropertyTag::Workable,
    PropertyTag::Structural,
    PropertyTag::Stockpile,
    PropertyTag::Settlement,
    PropertyTag::Farm,
    PropertyTag::Workshop,
];

const fn tag_mask(tag: PropertyTag) -> u64 {
    1u64 << (tag as u8)
}

impl From<MaterialType> for MaterialKind {
    fn from(value: MaterialType) -> Self {
        match value {
            MaterialType::Iron => MaterialKind::Iron,
            MaterialType::Steel => MaterialKind::Steel,
            MaterialType::Bronze => MaterialKind::Bronze,
            MaterialType::Leather => MaterialKind::Leather,
            MaterialType::Wood => MaterialKind::Wood,
            MaterialType::Bone => MaterialKind::Bone,
            MaterialType::Cloth => MaterialKind::Cloth,
            MaterialType::Stone => MaterialKind::Stone,
        }
    }
}
