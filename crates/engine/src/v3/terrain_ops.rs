use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::hex::{HEX_RADIUS, hex_to_world};
use super::spatial::{GeoMaterial, Heightfield, Vec2};
use crate::v2::hex::{Axial, offset_to_axial};

pub const COMPACTION_THRESHOLD: usize = 50;
pub const TERRAIN_PATCH_CELL_SIZE: f32 = 1.0;
pub const TERRAIN_PATCH_RADIUS: f32 = HEX_RADIUS * 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb2d {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb2d {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    pub fn around(center: Vec2, half_extents: Vec2) -> Self {
        Self {
            min: center - half_extents,
            max: center + half_extents,
        }
    }

    pub fn contains(&self, pos: Vec2) -> bool {
        pos.x >= self.min.x && pos.x <= self.max.x && pos.y >= self.min.y && pos.y <= self.max.y
    }

    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DitchProfile {
    Trapezoidal,
    VShaped,
    UShaped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WallProfile {
    Rounded,
    FlatTop,
    Pointed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TerrainRasterSpec {
    pub origin: Vec2,
    pub width: u32,
    pub height: u32,
    pub cell_size: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotData {
    pub origin: Vec2,
    pub cols: u16,
    pub rows: u16,
    pub cell_size: f32,
    pub deltas: Vec<f32>,
    pub materials: Option<Vec<GeoMaterial>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TerrainOp {
    Ditch {
        start: Vec2,
        end: Vec2,
        width: f32,
        depth: f32,
        profile: DitchProfile,
    },
    Wall {
        start: Vec2,
        end: Vec2,
        width: f32,
        height: f32,
        profile: WallProfile,
    },
    Crater {
        center: Vec2,
        radius: f32,
        depth: f32,
        rim_height: f32,
    },
    Road {
        points: SmallVec<[Vec2; 4]>,
        width: f32,
        grade: f32,
        material: GeoMaterial,
    },
    Flatten {
        center: Vec2,
        half_extents: Vec2,
        target_height: f32,
        rotation: f32,
    },
    Furrow {
        center: Vec2,
        half_extents: Vec2,
        rotation: f32,
        spacing: f32,
        depth: f32,
    },
    Bore {
        center: Vec2,
        radius: f32,
        depth: f32,
        angle: f32,
    },
    Snapshot(SnapshotData),
}

impl TerrainOp {
    pub fn bounds(&self) -> Aabb2d {
        match self {
            TerrainOp::Ditch {
                start, end, width, ..
            }
            | TerrainOp::Wall {
                start, end, width, ..
            } => segment_bounds(*start, *end, *width * 0.5),
            TerrainOp::Crater { center, radius, .. } | TerrainOp::Bore { center, radius, .. } => {
                Aabb2d::around(*center, Vec2::new(*radius, *radius))
            }
            TerrainOp::Road { points, width, .. } => polyline_bounds(points, *width * 0.5),
            TerrainOp::Flatten {
                center,
                half_extents,
                rotation,
                ..
            }
            | TerrainOp::Furrow {
                center,
                half_extents,
                rotation,
                ..
            } => rotated_rect_bounds(*center, *half_extents, *rotation),
            TerrainOp::Snapshot(snapshot) => Aabb2d::new(
                snapshot.origin,
                snapshot.origin
                    + Vec2::new(
                        snapshot.cols as f32 * snapshot.cell_size,
                        snapshot.rows as f32 * snapshot.cell_size,
                    ),
            ),
        }
    }

    pub fn evaluate_height_delta_at(&self, pos: Vec2) -> f32 {
        if !self.bounds().contains(pos) {
            return 0.0;
        }

        match self {
            TerrainOp::Ditch {
                start,
                end,
                width,
                depth,
                profile,
            } => evaluate_linear_cut(*start, *end, *width, *depth, *profile, pos),
            TerrainOp::Wall {
                start,
                end,
                width,
                height,
                profile,
            } => evaluate_linear_wall(*start, *end, *width, *height, *profile, pos),
            TerrainOp::Crater {
                center,
                radius,
                depth,
                rim_height,
            } => evaluate_crater(*center, *radius, *depth, *rim_height, pos),
            TerrainOp::Road { .. } => 0.0,
            TerrainOp::Flatten {
                center,
                half_extents,
                rotation,
                target_height,
            } => {
                let local = rotate_point(pos - *center, -*rotation);
                if local.x.abs() <= half_extents.x && local.y.abs() <= half_extents.y {
                    *target_height
                } else {
                    0.0
                }
            }
            TerrainOp::Furrow {
                center,
                half_extents,
                rotation,
                spacing,
                depth,
            } => evaluate_furrow(*center, *half_extents, *rotation, *spacing, *depth, pos),
            TerrainOp::Bore {
                center,
                radius,
                depth,
                angle,
            } => evaluate_bore(*center, *radius, *depth, *angle, pos),
            TerrainOp::Snapshot(snapshot) => sample_snapshot(snapshot, pos),
        }
    }

    pub fn material_override_at(&self, pos: Vec2) -> Option<GeoMaterial> {
        if !self.bounds().contains(pos) {
            return None;
        }

        match self {
            TerrainOp::Road {
                material,
                points,
                width,
                ..
            } => {
                let mut min_dist = f32::INFINITY;
                for window in points.windows(2) {
                    let (_, dist) = project_to_segment(pos, window[0], window[1]);
                    min_dist = min_dist.min(dist);
                }
                if min_dist <= *width * 0.5 {
                    Some(*material)
                } else {
                    None
                }
            }
            TerrainOp::Snapshot(snapshot) => sample_snapshot_material(snapshot, pos),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterizedPatch {
    pub origin: Vec2,
    pub cols: u16,
    pub rows: u16,
    pub cell_size: f32,
    pub heights: Vec<f32>,
    pub materials: Vec<GeoMaterial>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TerrainOpLog {
    stacks: HashMap<Axial, Vec<TerrainOp>>,
    #[serde(skip)]
    dirty_hexes: HashSet<Axial>,
    #[serde(skip)]
    cache: HashMap<Axial, RasterizedPatch>,
}

impl TerrainOpLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ops_for_hex(&self, hex: Axial) -> &[TerrainOp] {
        self.stacks.get(&hex).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn push_op(
        &mut self,
        hex: Axial,
        op: TerrainOp,
        base: &Heightfield,
        map_width: usize,
        map_height: usize,
    ) {
        let stack_len = {
            let stack = self.stacks.entry(hex).or_default();
            stack.push(op);
            stack.len()
        };
        self.invalidate(hex);
        if stack_len > COMPACTION_THRESHOLD {
            self.compact(hex, base, map_width, map_height);
        }
    }

    pub fn height_delta_at(&self, hex: Axial, pos: Vec2) -> f32 {
        self.ops_for_hex(hex)
            .iter()
            .fold(0.0, |acc, op| acc + op.evaluate_height_delta_at(pos))
    }

    pub fn material_override_at(&self, hex: Axial, pos: Vec2) -> Option<GeoMaterial> {
        self.ops_for_hex(hex)
            .iter()
            .rev()
            .find_map(|op| op.material_override_at(pos))
    }

    pub fn drain_dirty_hexes(&mut self) -> Vec<Axial> {
        let mut dirty: Vec<Axial> = self.dirty_hexes.drain().collect();
        dirty.sort_by_key(|hex| (hex.q, hex.r));
        dirty
    }

    pub fn rasterized_patch(
        &mut self,
        hex: Axial,
        base: &Heightfield,
        map_width: usize,
        map_height: usize,
    ) -> Option<&RasterizedPatch> {
        if !self.cache.contains_key(&hex) {
            let patch =
                rasterize_hex_patch(hex, self.ops_for_hex(hex), base, map_width, map_height);
            self.cache.insert(hex, patch);
        }
        self.cache.get(&hex)
    }

    fn invalidate(&mut self, hex: Axial) {
        self.cache.remove(&hex);
        self.dirty_hexes.insert(hex);
    }

    fn compact(&mut self, hex: Axial, base: &Heightfield, map_width: usize, map_height: usize) {
        let patch = rasterize_hex_patch(hex, self.ops_for_hex(hex), base, map_width, map_height);
        let coarse_base = patch
            .heights
            .iter()
            .enumerate()
            .map(|(idx, height)| {
                let x = idx as u32 % patch.cols as u32;
                let y = idx as u32 / patch.cols as u32;
                let pos = Vec2::new(
                    patch.origin.x + (x as f32 + 0.5) * patch.cell_size,
                    patch.origin.y + (y as f32 + 0.5) * patch.cell_size,
                );
                *height - sample_base_height(base, map_width, map_height, pos)
            })
            .collect();

        let snapshot = TerrainOp::Snapshot(SnapshotData {
            origin: patch.origin,
            cols: patch.cols,
            rows: patch.rows,
            cell_size: patch.cell_size,
            deltas: coarse_base,
            materials: Some(patch.materials.clone()),
        });

        self.stacks.insert(hex, vec![snapshot]);
        self.invalidate(hex);
    }
}

pub fn terrain_raster_spec(
    map_width: usize,
    map_height: usize,
    cell_size: f32,
) -> TerrainRasterSpec {
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    for row in 0..map_height as i32 {
        for col in 0..map_width as i32 {
            let center = hex_to_world(offset_to_axial(row, col)).xy();
            min.x = min.x.min(center.x - TERRAIN_PATCH_RADIUS);
            min.y = min.y.min(center.y - TERRAIN_PATCH_RADIUS);
            max.x = max.x.max(center.x + TERRAIN_PATCH_RADIUS);
            max.y = max.y.max(center.y + TERRAIN_PATCH_RADIUS);
        }
    }

    let width = ((max.x - min.x) / cell_size).ceil().max(1.0) as u32;
    let height = ((max.y - min.y) / cell_size).ceil().max(1.0) as u32;

    TerrainRasterSpec {
        origin: min,
        width,
        height,
        cell_size,
    }
}

pub fn rasterize_height_region(
    ops: &[TerrainOp],
    base: &Heightfield,
    map_width: usize,
    map_height: usize,
    origin: Vec2,
    width: u32,
    height: u32,
    cell_size: f32,
) -> RasterizedPatch {
    let mut heights = Vec::with_capacity((width * height) as usize);
    let mut materials = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let pos = Vec2::new(
                origin.x + (x as f32 + 0.5) * cell_size,
                origin.y + (y as f32 + 0.5) * cell_size,
            );
            let base_height = sample_base_height(base, map_width, map_height, pos);
            let delta = ops
                .iter()
                .fold(0.0, |acc, op| acc + op.evaluate_height_delta_at(pos));
            let material = ops
                .iter()
                .rev()
                .find_map(|op| op.material_override_at(pos))
                .unwrap_or_else(|| sample_base_material(base, map_width, map_height, pos));
            heights.push(base_height + delta);
            materials.push(material);
        }
    }

    RasterizedPatch {
        origin,
        cols: width as u16,
        rows: height as u16,
        cell_size,
        heights,
        materials,
    }
}

pub fn rasterize_hex_patch(
    hex: Axial,
    ops: &[TerrainOp],
    base: &Heightfield,
    map_width: usize,
    map_height: usize,
) -> RasterizedPatch {
    let center = hex_to_world(hex).xy();
    let origin = Vec2::new(
        center.x - TERRAIN_PATCH_RADIUS,
        center.y - TERRAIN_PATCH_RADIUS,
    );
    let size = (TERRAIN_PATCH_RADIUS * 2.0 / TERRAIN_PATCH_CELL_SIZE).ceil() as u32;
    rasterize_height_region(
        ops,
        base,
        map_width,
        map_height,
        origin,
        size,
        size,
        TERRAIN_PATCH_CELL_SIZE,
    )
}

pub fn sample_base_height(
    base: &Heightfield,
    map_width: usize,
    map_height: usize,
    pos: Vec2,
) -> f32 {
    base.effective_height_at(pos, |p| {
        super::hex::world_to_vertex(p, map_width, map_height)
    })
}

pub fn sample_base_material(
    base: &Heightfield,
    map_width: usize,
    map_height: usize,
    pos: Vec2,
) -> GeoMaterial {
    let (vx, vy) = super::hex::world_to_vertex(pos, map_width, map_height);
    let col = vx.round().clamp(0.0, (base.cols.saturating_sub(1)) as f32) as usize;
    let row = vy.round().clamp(0.0, (base.rows.saturating_sub(1)) as f32) as usize;
    base.vertex_at(col, row)
        .map(|vertex| vertex.material)
        .unwrap_or(GeoMaterial::Soil)
}

fn segment_bounds(start: Vec2, end: Vec2, radius: f32) -> Aabb2d {
    Aabb2d::new(
        Vec2::new(start.x.min(end.x) - radius, start.y.min(end.y) - radius),
        Vec2::new(start.x.max(end.x) + radius, start.y.max(end.y) + radius),
    )
}

fn polyline_bounds(points: &[Vec2], radius: f32) -> Aabb2d {
    let mut bounds = Aabb2d::new(points[0], points[0]);
    for point in &points[1..] {
        bounds = bounds.union(&Aabb2d::new(*point, *point));
    }
    Aabb2d::new(
        Vec2::new(bounds.min.x - radius, bounds.min.y - radius),
        Vec2::new(bounds.max.x + radius, bounds.max.y + radius),
    )
}

fn rotated_rect_bounds(center: Vec2, half_extents: Vec2, rotation: f32) -> Aabb2d {
    let corners = [
        Vec2::new(-half_extents.x, -half_extents.y),
        Vec2::new(half_extents.x, -half_extents.y),
        Vec2::new(half_extents.x, half_extents.y),
        Vec2::new(-half_extents.x, half_extents.y),
    ];

    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for corner in corners {
        let point = center + rotate_point(corner, rotation);
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
    }
    Aabb2d::new(min, max)
}

fn rotate_point(point: Vec2, angle: f32) -> Vec2 {
    let (sin_a, cos_a) = angle.sin_cos();
    Vec2::new(
        point.x * cos_a - point.y * sin_a,
        point.x * sin_a + point.y * cos_a,
    )
}

fn evaluate_linear_cut(
    start: Vec2,
    end: Vec2,
    width: f32,
    depth: f32,
    profile: DitchProfile,
    pos: Vec2,
) -> f32 {
    let (_, dist) = project_to_segment(pos, start, end);
    let radius = width * 0.5;
    if dist > radius {
        return 0.0;
    }
    let t = (dist / radius).clamp(0.0, 1.0);
    let shape = match profile {
        DitchProfile::Trapezoidal => {
            if t < 0.4 {
                1.0
            } else {
                1.0 - (t - 0.4) / 0.6
            }
        }
        DitchProfile::VShaped => 1.0 - t,
        DitchProfile::UShaped => 1.0 - t * t,
    };
    -depth * shape.max(0.0)
}

fn evaluate_linear_wall(
    start: Vec2,
    end: Vec2,
    width: f32,
    height: f32,
    profile: WallProfile,
    pos: Vec2,
) -> f32 {
    let (_, dist) = project_to_segment(pos, start, end);
    let radius = width * 0.5;
    if dist > radius {
        return 0.0;
    }
    let t = (dist / radius).clamp(0.0, 1.0);
    let shape = match profile {
        WallProfile::Rounded => 1.0 - t * t,
        WallProfile::FlatTop => {
            if t < 0.35 {
                1.0
            } else {
                1.0 - (t - 0.35) / 0.65
            }
        }
        WallProfile::Pointed => 1.0 - t,
    };
    height * shape.max(0.0)
}

fn evaluate_crater(center: Vec2, radius: f32, depth: f32, rim_height: f32, pos: Vec2) -> f32 {
    let dist = (pos - center).length();
    if dist > radius {
        return 0.0;
    }

    let inner = radius * 0.65;
    if dist <= inner {
        let t = dist / inner.max(1.0);
        -depth * (1.0 - t * t)
    } else {
        let outer_t = ((dist - inner) / (radius - inner).max(1.0)).clamp(0.0, 1.0);
        rim_height * (1.0 - outer_t)
    }
}

fn evaluate_furrow(
    center: Vec2,
    half_extents: Vec2,
    rotation: f32,
    spacing: f32,
    depth: f32,
    pos: Vec2,
) -> f32 {
    let local = rotate_point(pos - center, -rotation);
    if local.x.abs() > half_extents.x || local.y.abs() > half_extents.y {
        return 0.0;
    }
    let spacing = spacing.max(1.0);
    let lane = (local.x / spacing).round() * spacing;
    let dist = (local.x - lane).abs();
    let radius = spacing * 0.35;
    if dist > radius {
        0.0
    } else {
        -depth * (1.0 - dist / radius)
    }
}

fn evaluate_bore(center: Vec2, radius: f32, depth: f32, angle: f32, pos: Vec2) -> f32 {
    let dist = (pos - center).length();
    if dist > radius {
        return 0.0;
    }
    let slope_scale = angle.cos().abs().max(0.2);
    -depth * (1.0 - dist / radius) * slope_scale
}

fn sample_snapshot(snapshot: &SnapshotData, pos: Vec2) -> f32 {
    let rel = pos - snapshot.origin;
    if rel.x < 0.0 || rel.y < 0.0 {
        return 0.0;
    }
    let x = (rel.x / snapshot.cell_size).floor() as usize;
    let y = (rel.y / snapshot.cell_size).floor() as usize;
    if x >= snapshot.cols as usize || y >= snapshot.rows as usize {
        return 0.0;
    }
    snapshot.deltas[y * snapshot.cols as usize + x]
}

fn sample_snapshot_material(snapshot: &SnapshotData, pos: Vec2) -> Option<GeoMaterial> {
    let materials = snapshot.materials.as_ref()?;
    let rel = pos - snapshot.origin;
    if rel.x < 0.0 || rel.y < 0.0 {
        return None;
    }
    let x = (rel.x / snapshot.cell_size).floor() as usize;
    let y = (rel.y / snapshot.cell_size).floor() as usize;
    if x >= snapshot.cols as usize || y >= snapshot.rows as usize {
        return None;
    }
    materials.get(y * snapshot.cols as usize + x).copied()
}

fn project_to_segment(pos: Vec2, start: Vec2, end: Vec2) -> (f32, f32) {
    let segment = end - start;
    let len_sq = segment.length_squared();
    if len_sq <= 1e-6 {
        return (0.0, (pos - start).length());
    }
    let t = ((pos - start).dot(segment) / len_sq).clamp(0.0, 1.0);
    let projected = start + segment * t;
    (t, (pos - projected).length())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ditch_center_depth_matches() {
        let op = TerrainOp::Ditch {
            start: Vec2::new(0.0, 0.0),
            end: Vec2::new(10.0, 0.0),
            width: 4.0,
            depth: 2.0,
            profile: DitchProfile::Trapezoidal,
        };
        assert!((op.evaluate_height_delta_at(Vec2::new(5.0, 0.0)) + 2.0).abs() < 1e-4);
    }

    #[test]
    fn wall_center_height_matches() {
        let op = TerrainOp::Wall {
            start: Vec2::new(0.0, 0.0),
            end: Vec2::new(10.0, 0.0),
            width: 4.0,
            height: 1.5,
            profile: WallProfile::Rounded,
        };
        assert!((op.evaluate_height_delta_at(Vec2::new(5.0, 0.0)) - 1.5).abs() < 1e-4);
    }

    #[test]
    fn crater_has_center_and_rim() {
        let op = TerrainOp::Crater {
            center: Vec2::new(0.0, 0.0),
            radius: 10.0,
            depth: 3.0,
            rim_height: 1.0,
        };
        assert!(op.evaluate_height_delta_at(Vec2::new(0.0, 0.0)) < -2.9);
        assert!(op.evaluate_height_delta_at(Vec2::new(8.0, 0.0)) > 0.0);
    }
}
