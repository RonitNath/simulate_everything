use font8x8::UnicodeFonts;
use simulate_everything_engine::v3::behavior_snapshot::BehaviorSnapshot;
use simulate_everything_engine::v3::spatial::Vec2;
use simulate_everything_engine::v3::state::GameState;
use simulate_everything_engine::v3::terrain_ops::{TerrainOp, terrain_raster_spec};
use std::path::Path;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub const DEFAULT_RENDER_SIZE: u32 = 1024;

pub fn render_snapshot_png(
    state: &GameState,
    snapshot: &BehaviorSnapshot,
    path: &Path,
    size: u32,
) -> Result<(), String> {
    let mut pixmap =
        Pixmap::new(size, size).ok_or_else(|| "failed to allocate pixmap".to_string())?;
    pixmap.fill(Color::from_rgba8(242, 238, 229, 255));

    render_terrain(state, &mut pixmap);
    render_terrain_ops(state, &mut pixmap);
    render_entities(snapshot, &mut pixmap);
    draw_text(
        &mut pixmap,
        12,
        12,
        &format!("tick {}", snapshot.tick),
        Color::from_rgba8(32, 32, 32, 255),
        2,
    );

    pixmap
        .save_png(path)
        .map_err(|err| format!("save_png failed: {}", err))
}

fn render_terrain(state: &GameState, pixmap: &mut Pixmap) {
    let spec = terrain_raster_spec(state.map_width, state.map_height, 16.0);
    let mut paint = Paint::default();
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
            let px = (x as f32 / spec.width as f32) * pixmap.width() as f32;
            let py = (y as f32 / spec.height as f32) * pixmap.height() as f32;
            let pw = (pixmap.width() as f32 / spec.width as f32).ceil();
            let ph = (pixmap.height() as f32 / spec.height as f32).ceil();
            let rect = tiny_skia::Rect::from_xywh(px, py, pw, ph).unwrap();
            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }
}

fn render_terrain_ops(state: &GameState, pixmap: &mut Pixmap) {
    for row in 0..state.map_height as i32 {
        for col in 0..state.map_width as i32 {
            let hex = simulate_everything_engine::v2::hex::offset_to_axial(row, col);
            for op in state.terrain_ops.ops_for_hex(hex) {
                match op {
                    TerrainOp::Road { points, .. } if points.len() >= 2 => {
                        draw_polyline(pixmap, points, Color::from_rgba8(110, 110, 110, 220), 3.0);
                    }
                    TerrainOp::Ditch { start, end, .. } => {
                        draw_segment(
                            pixmap,
                            *start,
                            *end,
                            Color::from_rgba8(70, 115, 191, 220),
                            3.0,
                        );
                    }
                    TerrainOp::Wall { start, end, .. } => {
                        draw_segment(
                            pixmap,
                            *start,
                            *end,
                            Color::from_rgba8(120, 86, 56, 220),
                            4.0,
                        );
                    }
                    TerrainOp::Furrow {
                        center,
                        half_extents,
                        ..
                    } => {
                        let min = Vec2::new(center.x - half_extents.x, center.y - half_extents.y);
                        let max = Vec2::new(center.x + half_extents.x, center.y + half_extents.y);
                        draw_rect(pixmap, min, max, Color::from_rgba8(80, 150, 70, 100));
                    }
                    _ => {}
                }
            }
        }
    }
}

fn render_entities(snapshot: &BehaviorSnapshot, pixmap: &mut Pixmap) {
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
        let (sx, sy) = world_to_canvas(snapshot, pixmap, x, y);
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

fn draw_polyline(pixmap: &mut Pixmap, points: &[Vec2], color: Color, width: f32) {
    for window in points.windows(2) {
        draw_segment(pixmap, window[0], window[1], color, width);
    }
}

fn draw_segment(pixmap: &mut Pixmap, a: Vec2, b: Vec2, color: Color, width: f32) {
    let (ax, ay) = world_to_canvas_from_raw(pixmap, a.x, a.y);
    let (bx, by) = world_to_canvas_from_raw(pixmap, b.x, b.y);
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

fn draw_rect(pixmap: &mut Pixmap, min: Vec2, max: Vec2, color: Color) {
    let (x0, y0) = world_to_canvas_from_raw(pixmap, min.x, min.y);
    let (x1, y1) = world_to_canvas_from_raw(pixmap, max.x, max.y);
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

fn world_to_canvas(snapshot: &BehaviorSnapshot, pixmap: &Pixmap, x: f32, y: f32) -> (f32, f32) {
    let nx = x / (snapshot.map_width.max(1) as f32 * 100.0);
    let ny = y / (snapshot.map_height.max(1) as f32 * 100.0);
    (nx * pixmap.width() as f32, ny * pixmap.height() as f32)
}

fn world_to_canvas_from_raw(pixmap: &Pixmap, x: f32, y: f32) -> (f32, f32) {
    let nx = ((x + 1000.0) / 2000.0).clamp(0.0, 1.0);
    let ny = ((y + 1000.0) / 2000.0).clamp(0.0, 1.0);
    (nx * pixmap.width() as f32, ny * pixmap.height() as f32)
}
