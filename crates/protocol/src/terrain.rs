use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainRasterInit {
    pub width: u32,
    pub height: u32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell_size: f32,
    pub heights: Vec<f32>,
    pub materials: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainPatch {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub heights: Vec<f32>,
    pub materials: Vec<u32>,
}
