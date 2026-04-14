use font8x8::UnicodeFonts;
use simulate_everything_engine::v3::behavior_snapshot::BehaviorSnapshot;
use simulate_everything_engine::v3::spatial::Vec2;
use simulate_everything_engine::v3::state::GameState;
use simulate_everything_engine::v3::terrain_ops::{TerrainOp, terrain_raster_spec};
use std::path::Path;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

pub const DEFAULT_RENDER_SIZE: u32 = 960;
const TERRAIN_CELL_SIZE: f32 = 16.0;

#[derive(Debug, Clone, Copy)]
struct Projection {
    origin: Vec2,
    width: f32,
    height: f32,
    canvas_size: u32,
}

impl Projection {
    fn for_map(map_width: usize, map_height: usize, canvas_size: u32) -> Self {
        let spec = terrain_raster_spec(map_width, map_height, TERRAIN_CELL_SIZE);
        Self {
            origin: spec.origin,
            width: (spec.width as f32 * spec.cell_size).max(1.0),
            height: (spec.height as f32 * spec.cell_size).max(1.0),
            canvas_size,
        }
    }

    fn project(&self, x: f32, y: f32) -> (f32, f32) {
        let nx = ((x - self.origin.x) / self.width).clamp(0.0, 1.0);
        let ny = ((y - self.origin.y) / self.height).clamp(0.0, 1.0);
        (nx * self.canvas_size as f32, ny * self.canvas_size as f32)
    }
}

fn background_color() -> Color {
    Color::from_rgba8(242, 238, 229, 255)
}

pub struct CachedRenderer {
    size: u32,
    terrain_background: Option<Pixmap>,
    terrain_ops_revision: Option<u64>,
    terrain_ops_overlay: Option<Pixmap>,
}

impl CachedRenderer {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            terrain_background: None,
            terrain_ops_revision: None,
            terrain_ops_overlay: None,
        }
    }

    pub fn render_snapshot_png(
        &mut self,
        state: &GameState,
        snapshot: &BehaviorSnapshot,
        path: &Path,
    ) -> Result<(), String> {
        let projection = Projection::for_map(state.map_width, state.map_height, self.size);
        let terrain_revision = state.terrain_ops.revision();
        if self.terrain_background.is_none() {
            self.terrain_background = Some(render_terrain_background(state, self.size)?);
        }
        if self.terrain_ops_revision != Some(terrain_revision) || self.terrain_ops_overlay.is_none()
        {
            self.terrain_ops_overlay =
                Some(render_terrain_ops_overlay(state, self.size, projection)?);
            self.terrain_ops_revision = Some(terrain_revision);
        }

        let mut frame = Pixmap::new(self.size, self.size)
            .ok_or_else(|| "failed to allocate pixmap".to_string())?;
        frame.fill(background_color());
        if let Some(background) = &self.terrain_background {
            frame.draw_pixmap(
                0,
                0,
                background.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        if let Some(terrain_ops) = &self.terrain_ops_overlay {
            frame.draw_pixmap(
                0,
                0,
                terrain_ops.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }

        let overlay = render_entities_overlay(snapshot, self.size, projection)?;
        frame.draw_pixmap(
            0,
            0,
            overlay.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        draw_text(
            &mut frame,
            12,
            12,
            &format!("tick {}", snapshot.tick),
            Color::from_rgba8(32, 32, 32, 255),
            2,
        );

        frame
            .save_png(path)
            .map_err(|err| format!("save_png failed: {}", err))
    }
}

fn render_terrain_background(state: &GameState, size: u32) -> Result<Pixmap, String> {
    let mut pixmap =
        Pixmap::new(size, size).ok_or_else(|| "failed to allocate pixmap".to_string())?;
    pixmap.fill(background_color());

    let spec = terrain_raster_spec(state.map_width, state.map_height, TERRAIN_CELL_SIZE);
    let mut paint = Paint::default();
    let px_w = (pixmap.width() as f32 / spec.width.max(1) as f32)
        .ceil()
        .max(1.0);
    let px_h = (pixmap.height() as f32 / spec.height.max(1) as f32)
        .ceil()
        .max(1.0);
    for y in 0..spec.height {
        for x in 0..spec.width {
            let world = Vec2::new(
                spec.origin.x + (x as f32 + 0.5) * spec.cell_size,
                spec.origin.y + (y as f32 + 0.5) * spec.cell_size,
            );
            let h = simulate_everything_engine::v3::terrain_ops::sample_base_height(
                &state.heightfield,
                state.map_width,
                state.map_height,
                world,
            );
            let shade = (((h + 20.0) / 40.0).clamp(0.0, 1.0) * 255.0) as u8;
            paint.set_color(Color::from_rgba8(shade, shade, shade, 255));
            let px = (x as f32 / spec.width.max(1) as f32) * pixmap.width() as f32;
            let py = (y as f32 / spec.height.max(1) as f32) * pixmap.height() as f32;
            let rect = tiny_skia::Rect::from_xywh(px, py, px_w, px_h).unwrap();
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }
    Ok(pixmap)
}

fn render_terrain_ops_overlay(
    state: &GameState,
    size: u32,
    projection: Projection,
) -> Result<Pixmap, String> {
    let mut pixmap =
        Pixmap::new(size, size).ok_or_else(|| "failed to allocate pixmap".to_string())?;
    render_terrain_ops(state, &mut pixmap, projection);
    Ok(pixmap)
}

fn render_entities_overlay(
    snapshot: &BehaviorSnapshot,
    size: u32,
    projection: Projection,
) -> Result<Pixmap, String> {
    let mut pixmap =
        Pixmap::new(size, size).ok_or_else(|| "failed to allocate pixmap".to_string())?;
    render_entities(snapshot, &mut pixmap, projection);
    Ok(pixmap)
}

fn render_terrain_ops(state: &GameState, pixmap: &mut Pixmap, projection: Projection) {
    for row in 0..state.map_height as i32 {
        for col in 0..state.map_width as i32 {
            let hex = simulate_everything_engine::v2::hex::offset_to_axial(row, col);
            for op in state.terrain_ops.ops_for_hex(hex) {
                match op {
                    TerrainOp::Road { points, .. } if points.len() >= 2 => {
                        draw_polyline(
                            pixmap,
                            points,
                            Color::from_rgba8(110, 110, 110, 220),
                            3.0,
                            projection,
                        );
                    }
                    TerrainOp::Ditch { start, end, .. } => {
                        draw_segment(
                            pixmap,
                            *start,
                            *end,
                            Color::from_rgba8(70, 115, 191, 220),
                            3.0,
                            projection,
                        );
                    }
                    TerrainOp::Wall { start, end, .. } => {
                        draw_segment(
                            pixmap,
                            *start,
                            *end,
                            Color::from_rgba8(120, 86, 56, 220),
                            4.0,
                            projection,
                        );
                    }
                    TerrainOp::Furrow {
                        center,
                        half_extents,
                        ..
                    } => {
                        let min = Vec2::new(center.x - half_extents.x, center.y - half_extents.y);
                        let max = Vec2::new(center.x + half_extents.x, center.y + half_extents.y);
                        draw_rect(
                            pixmap,
                            min,
                            max,
                            Color::from_rgba8(80, 150, 70, 100),
                            projection,
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn render_entities(snapshot: &BehaviorSnapshot, pixmap: &mut Pixmap, projection: Projection) {
    for entity in &snapshot.entities {
        let owner = entity.owner.unwrap_or(7);
        let color = match owner % 6 {
            0 => Color::from_rgba8(58, 110, 255, 255),
            1 => Color::from_rgba8(228, 84, 94, 255),
            2 => Color::from_rgba8(46, 175, 97, 255),
            3 => Color::from_rgba8(242, 201, 76, 255),
            4 => Color::from_rgba8(187, 107, 217, 255),
            _ => Color::from_rgba8(59, 201, 219, 255),
        };
        let [x, y, _] = entity.pos;
        let (sx, sy) = projection.project(x, y);
        let mut paint = Paint::default();
        paint.set_color(color);
        let circle = tiny_skia::PathBuilder::from_circle(sx, sy, 5.0).unwrap();
        pixmap.fill_path(
            &circle,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );

        if let Some(goal) = entity.current_goal.as_ref() {
            draw_text(pixmap, sx as i32 + 8, sy as i32 - 8, goal, color, 1);
        }
        if let Some(action) = entity.current_action.as_ref() {
            draw_text(
                pixmap,
                sx as i32 + 8,
                sy as i32 + 4,
                action,
                Color::from_rgba8(20, 20, 20, 255),
                1,
            );
        }
    }
}

fn draw_polyline(
    pixmap: &mut Pixmap,
    points: &[Vec2],
    color: Color,
    width: f32,
    projection: Projection,
) {
    for window in points.windows(2) {
        draw_segment(pixmap, window[0], window[1], color, width, projection);
    }
}

fn draw_segment(
    pixmap: &mut Pixmap,
    a: Vec2,
    b: Vec2,
    color: Color,
    width: f32,
    projection: Projection,
) {
    let (ax, ay) = projection.project(a.x, a.y);
    let (bx, by) = projection.project(b.x, b.y);
    let mut pb = PathBuilder::new();
    pb.move_to(ax, ay);
    pb.line_to(bx, by);
    let path = pb.finish().unwrap();
    let mut paint = Paint::default();
    paint.set_color(color);
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn draw_rect(pixmap: &mut Pixmap, min: Vec2, max: Vec2, color: Color, projection: Projection) {
    let (x0, y0) = projection.project(min.x, min.y);
    let (x1, y1) = projection.project(max.x, max.y);
    let rect = tiny_skia::Rect::from_ltrb(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)).unwrap();
    let mut paint = Paint::default();
    paint.set_color(color);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn draw_text(pixmap: &mut Pixmap, x: i32, y: i32, text: &str, color: Color, scale: u32) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(bitmap) = font8x8::BASIC_FONTS.get(ch) {
            for (row, bits) in bitmap.iter().enumerate() {
                for col in 0..8 {
                    if bits & (1 << col) == 0 {
                        continue;
                    }
                    let px = x + (i as i32 * 8 + col) * scale as i32;
                    let py = y + row as i32 * scale as i32;
                    let rect = tiny_skia::Rect::from_xywh(
                        px as f32,
                        py as f32,
                        scale as f32,
                        scale as f32,
                    );
                    if let Some(rect) = rect {
                        let mut paint = Paint::default();
                        paint.set_color(color);
                        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                    }
                }
            }
        }
    }
}
