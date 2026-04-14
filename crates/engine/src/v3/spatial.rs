use std::collections::HashMap;
use std::ops::{Add, Mul, Sub};

use serde::{Deserialize, Serialize};

/// 3D world-space position. f32 gives sub-mm precision at 30km map extent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// 2D position (x, y), ignoring altitude.
    pub fn xy(self) -> Vec2 {
        Vec2 {
            x: self.x,
            y: self.y,
        }
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len < 1e-10 {
            return Self::ZERO;
        }
        Self {
            x: self.x / len,
            y: self.y / len,
            z: self.z / len,
        }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
}

/// 2D position in the horizontal plane.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len < 1e-10 {
            return Self::ZERO;
        }
        Self {
            x: self.x / len,
            y: self.y / len,
        }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// Perpendicular dot product (2D cross product).
    pub fn perp_dot(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
        }
    }
}

// ---------------------------------------------------------------------------
// Geological material
// ---------------------------------------------------------------------------

/// Geological material at a vertex. Determines dig speed and base friction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeoMaterial {
    Soil,
    Sand,
    Clay,
    Rock,
}

impl GeoMaterial {
    /// Base friction multiplier for walking. Higher = slower.
    pub fn friction(self) -> f32 {
        match self {
            GeoMaterial::Soil => 1.0,
            GeoMaterial::Sand => 1.3,
            GeoMaterial::Clay => 1.1,
            GeoMaterial::Rock => 0.9,
        }
    }
}

// ---------------------------------------------------------------------------
// Vertex heightfield
// ---------------------------------------------------------------------------

/// Unique identifier for a heightfield vertex (flat index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VertexId(pub u32);

/// A single vertex in the heightfield.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Vertex {
    pub height: f32,
    pub material: GeoMaterial,
}

/// Mutable vertex heightfield. The ground is a flat array of vertices,
/// offset-indexed. Each hex corner maps to a vertex via pure math.
///
/// The heightfield has `cols` × `rows` vertices. For a hex map of
/// `map_width` × `map_height` hexes, vertex count ≈ 2 × hex count + boundary.
///
/// Base heights come from mapgen (reproducible from seed). Runtime mutations
/// are tracked in a sparse delta map for replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heightfield {
    pub cols: usize,
    pub rows: usize,
    vertices: Vec<Vertex>,
    /// Accumulated deltas for replay. Only modified vertices are stored.
    deltas: HashMap<u32, f32>,
}

impl Heightfield {
    /// Create a flat heightfield at the given base height.
    pub fn new(cols: usize, rows: usize, base_height: f32, material: GeoMaterial) -> Self {
        let count = cols * rows;
        Self {
            cols,
            rows,
            vertices: vec![
                Vertex {
                    height: base_height,
                    material,
                };
                count
            ],
            deltas: HashMap::new(),
        }
    }

    /// Create from a pre-built vertex array (mapgen output).
    pub fn from_vertices(cols: usize, rows: usize, vertices: Vec<Vertex>) -> Self {
        assert_eq!(
            vertices.len(),
            cols * rows,
            "vertex count must match cols * rows"
        );
        Self {
            cols,
            rows,
            vertices,
            deltas: HashMap::new(),
        }
    }

    fn flat_index(&self, col: usize, row: usize) -> Option<usize> {
        if col < self.cols && row < self.rows {
            Some(row * self.cols + col)
        } else {
            None
        }
    }

    pub fn vertex_at(&self, col: usize, row: usize) -> Option<&Vertex> {
        self.flat_index(col, row).map(|i| &self.vertices[i])
    }

    pub fn vertex_at_mut(&mut self, col: usize, row: usize) -> Option<&mut Vertex> {
        self.flat_index(col, row).map(|i| &mut self.vertices[i])
    }

    /// Mutate a vertex height. Records the delta for replay.
    pub fn modify_vertex(&mut self, id: VertexId, delta: f32) {
        let idx = id.0 as usize;
        if idx < self.vertices.len() {
            self.vertices[idx].height += delta;
            *self.deltas.entry(id.0).or_insert(0.0) += delta;
        }
    }

    /// All accumulated deltas since base state.
    pub fn deltas(&self) -> &HashMap<u32, f32> {
        &self.deltas
    }

    /// Clear deltas (after persisting to replay).
    pub fn clear_deltas(&mut self) {
        self.deltas.clear();
    }

    /// Interpolated height at a continuous world position using barycentric
    /// interpolation of the 3 nearest vertices.
    ///
    /// `pos_to_vertex` converts a world Vec2 to fractional vertex grid
    /// coordinates. The caller provides this because the mapping depends on
    /// hex geometry (see `hex.rs`).
    pub fn effective_height_at<F>(&self, pos: Vec2, pos_to_vertex: F) -> f32
    where
        F: Fn(Vec2) -> (f32, f32),
    {
        let (vx, vy) = pos_to_vertex(pos);

        // Clamp to grid bounds
        let vx = vx.clamp(0.0, (self.cols - 1) as f32);
        let vy = vy.clamp(0.0, (self.rows - 1) as f32);

        let col0 = vx.floor() as usize;
        let row0 = vy.floor() as usize;
        let col1 = (col0 + 1).min(self.cols - 1);
        let row1 = (row0 + 1).min(self.rows - 1);

        let fx = vx - col0 as f32;
        let fy = vy - row0 as f32;

        // Bilinear interpolation (simpler and sufficient for smooth terrain).
        // True barycentric on triangulated quads would split along the diagonal,
        // but bilinear avoids visible seams.
        let h00 = self.vertices[row0 * self.cols + col0].height;
        let h10 = self.vertices[row0 * self.cols + col1].height;
        let h01 = self.vertices[row1 * self.cols + col0].height;
        let h11 = self.vertices[row1 * self.cols + col1].height;

        let h0 = h00 + (h10 - h00) * fx;
        let h1 = h01 + (h11 - h01) * fx;
        h0 + (h1 - h0) * fy
    }

    /// Directional slope at a position. Returns rise/run in the given direction.
    pub fn slope_at<F>(&self, pos: Vec2, direction: Vec2, pos_to_vertex: F) -> f32
    where
        F: Fn(Vec2) -> (f32, f32),
    {
        let sample_dist = 1.0; // 1 meter sample
        let dir = direction.normalize();
        if dir.length_squared() < 1e-10 {
            return 0.0;
        }
        let p0 = pos;
        let p1 = Vec2::new(pos.x + dir.x * sample_dist, pos.y + dir.y * sample_dist);
        let h0 = self.effective_height_at(p0, &pos_to_vertex);
        let h1 = self.effective_height_at(p1, &pos_to_vertex);
        (h1 - h0) / sample_dist
    }

    /// Material at a vertex.
    pub fn material_at(&self, id: VertexId) -> Option<GeoMaterial> {
        self.vertices.get(id.0 as usize).map(|v| v.material)
    }

    /// Total vertex count.
    pub fn len(&self) -> usize {
        self.vertices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Layer (derived from z vs terrain height)
// ---------------------------------------------------------------------------

/// Spatial layer, derived from entity z relative to terrain height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Layer {
    Underground(u8),
    Surface,
    Air(u8),
}

/// Derive the layer from entity z and terrain z at that position.
pub fn layer_of(entity_z: f32, terrain_z: f32) -> Layer {
    let diff = entity_z - terrain_z;
    if diff < -0.5 {
        // Underground: depth tiers at 5m intervals
        let depth = ((-diff - 0.5) / 5.0).floor() as u8;
        Layer::Underground(depth.saturating_add(1))
    } else if diff > 2.0 {
        // Air: altitude tiers at 10m intervals
        let alt = ((diff - 2.0) / 10.0).floor() as u8;
        Layer::Air(alt.saturating_add(1))
    } else {
        Layer::Surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec3_basic_ops() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        let sum = a + b;
        assert!((sum.x - 5.0).abs() < 1e-6);
        assert!((sum.y - 7.0).abs() < 1e-6);
        assert!((sum.z - 9.0).abs() < 1e-6);

        let diff = b - a;
        assert!((diff.x - 3.0).abs() < 1e-6);

        let scaled = a * 2.0;
        assert!((scaled.x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn vec3_normalize() {
        let v = Vec3::new(3.0, 0.0, 4.0);
        let n = v.normalize();
        assert!((n.length() - 1.0).abs() < 1e-6);
        assert!((n.x - 0.6).abs() < 1e-6);
        assert!((n.z - 0.8).abs() < 1e-6);
    }

    #[test]
    fn vec3_zero_normalize() {
        let v = Vec3::ZERO;
        let n = v.normalize();
        assert_eq!(n, Vec3::ZERO);
    }

    #[test]
    fn vec2_perp_dot() {
        let a = Vec2::new(1.0, 0.0);
        let b = Vec2::new(0.0, 1.0);
        assert!((a.perp_dot(b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn heightfield_flat() {
        let hf = Heightfield::new(4, 4, 10.0, GeoMaterial::Soil);
        let identity = |p: Vec2| (p.x, p.y);
        let h = hf.effective_height_at(Vec2::new(1.5, 1.5), identity);
        assert!((h - 10.0).abs() < 1e-6);
    }

    #[test]
    fn heightfield_interpolation() {
        let mut verts = vec![
            Vertex {
                height: 0.0,
                material: GeoMaterial::Soil,
            };
            4
        ];
        // 2x2 grid: bottom-left 0, bottom-right 10, top-left 0, top-right 10
        verts[0].height = 0.0; // (0,0)
        verts[1].height = 10.0; // (1,0)
        verts[2].height = 0.0; // (0,1)
        verts[3].height = 10.0; // (1,1)

        let hf = Heightfield::from_vertices(2, 2, verts);
        let identity = |p: Vec2| (p.x, p.y);

        // Midpoint should be 5.0
        let h = hf.effective_height_at(Vec2::new(0.5, 0.5), identity);
        assert!((h - 5.0).abs() < 1e-4);

        // At (1.0, 0.5) should be 10.0
        let h = hf.effective_height_at(Vec2::new(1.0, 0.5), identity);
        assert!((h - 10.0).abs() < 1e-4);
    }

    #[test]
    fn heightfield_modify_and_deltas() {
        let mut hf = Heightfield::new(3, 3, 5.0, GeoMaterial::Rock);
        let vid = VertexId(4); // center vertex
        hf.modify_vertex(vid, 3.0);
        hf.modify_vertex(vid, -1.0);

        assert!((hf.vertices[4].height - 7.0).abs() < 1e-6);
        assert!((hf.deltas()[&4] - 2.0).abs() < 1e-6);

        hf.clear_deltas();
        assert!(hf.deltas().is_empty());
    }

    #[test]
    fn slope_flat_terrain() {
        let hf = Heightfield::new(4, 4, 10.0, GeoMaterial::Soil);
        let identity = |p: Vec2| (p.x, p.y);
        let slope = hf.slope_at(Vec2::new(1.0, 1.0), Vec2::new(1.0, 0.0), identity);
        assert!(slope.abs() < 1e-4);
    }

    #[test]
    fn layer_derivation() {
        assert_eq!(layer_of(10.0, 10.0), Layer::Surface);
        assert_eq!(layer_of(10.5, 10.0), Layer::Surface);
        assert_eq!(layer_of(13.0, 10.0), Layer::Air(1));
        assert_eq!(layer_of(3.0, 10.0), Layer::Underground(2)); // 7m deep, tier 2
        assert_eq!(layer_of(9.0, 10.0), Layer::Underground(1)); // 1m deep, tier 1
    }

    #[test]
    fn geo_material_friction() {
        assert!(GeoMaterial::Rock.friction() < GeoMaterial::Sand.friction());
        assert!(GeoMaterial::Soil.friction() < GeoMaterial::Clay.friction());
    }
}
