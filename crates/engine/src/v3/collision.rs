use super::spatial::Vec2;

// ---------------------------------------------------------------------------
// Geometry primitives
// ---------------------------------------------------------------------------

/// Collision geometry variants carried by entities with physical presence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geometry {
    Circle(Circle),
    Segment(LineSegment),
    Rect(OrientedRect),
    Triangle(Triangle),
}

/// Circle primitive — people, animals, trees, round structures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pub center: Vec2,
    pub radius: f32,
}

/// Line segment with thickness — walls, fences, palisades.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineSegment {
    pub start: Vec2,
    pub end: Vec2,
    pub thickness: f32,
}

/// Oriented rectangle — buildings, wagons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrientedRect {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub rotation: f32, // radians
}

/// Triangle — bastion fortifications, star forts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub a: Vec2,
    pub b: Vec2,
    pub c: Vec2,
}

// ---------------------------------------------------------------------------
// Hit result for ray/arc queries
// ---------------------------------------------------------------------------

/// Intersection result from a ray or arc query.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    /// Distance along the ray/arc from origin.
    pub t: f32,
    /// World position of the intersection.
    pub point: Vec2,
    /// Surface normal at the intersection point (pointing away from the surface).
    pub normal: Vec2,
}

// ---------------------------------------------------------------------------
// Pairwise intersection tests
// ---------------------------------------------------------------------------

/// Test if two geometry primitives overlap (2D).
pub fn intersects(a: &Geometry, b: &Geometry) -> bool {
    match (a, b) {
        (Geometry::Circle(a), Geometry::Circle(b)) => circle_circle(a, b),
        (Geometry::Circle(c), Geometry::Segment(s)) | (Geometry::Segment(s), Geometry::Circle(c)) => {
            circle_segment(c, s)
        }
        (Geometry::Circle(c), Geometry::Rect(r)) | (Geometry::Rect(r), Geometry::Circle(c)) => {
            circle_rect(c, r)
        }
        (Geometry::Circle(c), Geometry::Triangle(t))
        | (Geometry::Triangle(t), Geometry::Circle(c)) => circle_triangle(c, t),
        (Geometry::Segment(a), Geometry::Segment(b)) => segment_segment(a, b),
        (Geometry::Segment(s), Geometry::Rect(r)) | (Geometry::Rect(r), Geometry::Segment(s)) => {
            segment_rect(s, r)
        }
        (Geometry::Segment(s), Geometry::Triangle(t))
        | (Geometry::Triangle(t), Geometry::Segment(s)) => segment_triangle(s, t),
        (Geometry::Rect(a), Geometry::Rect(b)) => rect_rect(a, b),
        (Geometry::Rect(r), Geometry::Triangle(t))
        | (Geometry::Triangle(t), Geometry::Rect(r)) => rect_triangle(r, t),
        (Geometry::Triangle(a), Geometry::Triangle(b)) => triangle_triangle(a, b),
    }
}

// ---------------------------------------------------------------------------
// Circle-Circle
// ---------------------------------------------------------------------------

fn circle_circle(a: &Circle, b: &Circle) -> bool {
    let dx = b.center.x - a.center.x;
    let dy = b.center.y - a.center.y;
    let dist_sq = dx * dx + dy * dy;
    let radii = a.radius + b.radius;
    dist_sq <= radii * radii
}

// ---------------------------------------------------------------------------
// Circle-Segment
// ---------------------------------------------------------------------------

fn circle_segment(c: &Circle, s: &LineSegment) -> bool {
    let dist = point_segment_distance(c.center, s.start, s.end);
    dist <= c.radius + s.thickness / 2.0
}

// ---------------------------------------------------------------------------
// Circle-Rect
// ---------------------------------------------------------------------------

fn circle_rect(c: &Circle, r: &OrientedRect) -> bool {
    // Transform circle center into rect's local space
    let local = to_rect_local(c.center, r);
    // Clamp to rect bounds
    let closest = Vec2::new(
        local.x.clamp(-r.half_extents.x, r.half_extents.x),
        local.y.clamp(-r.half_extents.y, r.half_extents.y),
    );
    let d = local - closest;
    d.length_squared() <= c.radius * c.radius
}

// ---------------------------------------------------------------------------
// Circle-Triangle
// ---------------------------------------------------------------------------

fn circle_triangle(c: &Circle, t: &Triangle) -> bool {
    // Check if circle center is inside triangle
    if point_in_triangle(c.center, t) {
        return true;
    }
    // Check distance to each edge
    let edges = [(t.a, t.b), (t.b, t.c), (t.c, t.a)];
    for (s, e) in &edges {
        if point_segment_distance(c.center, *s, *e) <= c.radius {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Segment-Segment
// ---------------------------------------------------------------------------

fn segment_segment(a: &LineSegment, b: &LineSegment) -> bool {
    // Check if the centerlines are within combined thickness
    let dist = segment_segment_distance(a.start, a.end, b.start, b.end);
    dist <= (a.thickness + b.thickness) / 2.0
}

// ---------------------------------------------------------------------------
// Segment-Rect
// ---------------------------------------------------------------------------

fn segment_rect(s: &LineSegment, r: &OrientedRect) -> bool {
    // Transform segment endpoints into rect local space, test against AABB
    let ls = to_rect_local(s.start, r);
    let le = to_rect_local(s.end, r);

    let half = r.half_extents;
    let expand = s.thickness / 2.0;
    let expanded = Vec2::new(half.x + expand, half.y + expand);

    // Test segment vs expanded AABB in local space
    segment_aabb(ls, le, expanded)
}

// ---------------------------------------------------------------------------
// Segment-Triangle
// ---------------------------------------------------------------------------

fn segment_triangle(s: &LineSegment, t: &Triangle) -> bool {
    // Check if any point of segment is inside triangle (within thickness)
    if point_in_triangle(s.start, t) || point_in_triangle(s.end, t) {
        return true;
    }
    // Check segment against each triangle edge
    let edges = [(t.a, t.b), (t.b, t.c), (t.c, t.a)];
    for (e_start, e_end) in &edges {
        let dist = segment_segment_distance(s.start, s.end, *e_start, *e_end);
        if dist <= s.thickness / 2.0 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Rect-Rect
// ---------------------------------------------------------------------------

fn rect_rect(a: &OrientedRect, b: &OrientedRect) -> bool {
    // SAT with 4 axes (2 per rect)
    let axes = rect_axes(a)
        .into_iter()
        .chain(rect_axes(b));
    let corners_a = rect_corners(a);
    let corners_b = rect_corners(b);

    for axis in axes {
        let (min_a, max_a) = project_corners(&corners_a, axis);
        let (min_b, max_b) = project_corners(&corners_b, axis);
        if max_a < min_b || max_b < min_a {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Rect-Triangle
// ---------------------------------------------------------------------------

fn rect_triangle(r: &OrientedRect, t: &Triangle) -> bool {
    // SAT with rect axes + triangle edge normals
    let corners_r = rect_corners(r);
    let corners_t = [t.a, t.b, t.c];

    let tri_edges = [t.b - t.a, t.c - t.b, t.a - t.c];
    let axes = rect_axes(r)
        .into_iter()
        .chain(tri_edges.iter().map(|e| Vec2::new(-e.y, e.x)));

    for axis in axes {
        if axis.length_squared() < 1e-10 {
            continue;
        }
        let (min_r, max_r) = project_corners(&corners_r, axis);
        let (min_t, max_t) = project_corners(&corners_t, axis);
        if max_r < min_t || max_t < min_r {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Triangle-Triangle
// ---------------------------------------------------------------------------

fn triangle_triangle(a: &Triangle, b: &Triangle) -> bool {
    let corners_a = [a.a, a.b, a.c];
    let corners_b = [b.a, b.b, b.c];

    let edges_a = [a.b - a.a, a.c - a.b, a.a - a.c];
    let edges_b = [b.b - b.a, b.c - b.b, b.a - b.c];

    let axes = edges_a
        .iter()
        .chain(edges_b.iter())
        .map(|e| Vec2::new(-e.y, e.x));

    for axis in axes {
        if axis.length_squared() < 1e-10 {
            continue;
        }
        let (min_a, max_a) = project_corners(&corners_a, axis);
        let (min_b, max_b) = project_corners(&corners_b, axis);
        if max_a < min_b || max_b < min_a {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Ray intersection per primitive
// ---------------------------------------------------------------------------

/// Ray-circle intersection. Returns the nearest hit distance along the ray, if any.
pub fn ray_circle(origin: Vec2, dir: Vec2, circle: &Circle) -> Option<Hit> {
    let oc = origin - circle.center;
    let a = dir.dot(dir);
    let b = 2.0 * oc.dot(dir);
    let c = oc.dot(oc) - circle.radius * circle.radius;
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    let sqrt_d = discriminant.sqrt();
    let t = (-b - sqrt_d) / (2.0 * a);
    if t < 0.0 {
        // Try the far intersection
        let t2 = (-b + sqrt_d) / (2.0 * a);
        if t2 < 0.0 {
            return None;
        }
        let point = origin + dir * t2;
        let normal = (point - circle.center).normalize();
        return Some(Hit {
            t: t2,
            point,
            normal,
        });
    }
    let point = origin + dir * t;
    let normal = (point - circle.center).normalize();
    Some(Hit { t, point, normal })
}

/// Ray-segment intersection (treating segment as a thick line).
pub fn ray_segment(origin: Vec2, dir: Vec2, seg: &LineSegment) -> Option<Hit> {
    // For thickness, we treat the segment as a capsule (circle at each end + rectangle body).
    // Simplified: test ray against the centerline, then check distance <= thickness/2.

    let seg_dir = seg.end - seg.start;
    let denom = dir.perp_dot(seg_dir);
    if denom.abs() < 1e-10 {
        // Parallel — check if within thickness
        let dist = point_segment_distance(origin, seg.start, seg.end);
        if dist <= seg.thickness / 2.0 {
            // Origin is inside the segment's thickness band
            return Some(Hit {
                t: 0.0,
                point: origin,
                normal: Vec2::new(-seg_dir.y, seg_dir.x).normalize(),
            });
        }
        return None;
    }

    let diff = seg.start - origin;
    let t = diff.perp_dot(seg_dir) / denom;
    let u = diff.perp_dot(dir) / denom;

    if t >= 0.0 && (0.0..=1.0).contains(&u) {
        let point = origin + dir * t;
        // Check if the hit point is within thickness
        let dist = point_segment_distance(point, seg.start, seg.end);
        if dist <= seg.thickness / 2.0 + 1e-4 {
            let normal = Vec2::new(-seg_dir.y, seg_dir.x).normalize();
            // Flip normal to face the ray origin
            let normal = if normal.dot(dir) > 0.0 {
                normal * -1.0
            } else {
                normal
            };
            return Some(Hit { t, point, normal });
        }
    }
    None
}

/// Ray-oriented rect intersection.
pub fn ray_rect(origin: Vec2, dir: Vec2, rect: &OrientedRect) -> Option<Hit> {
    // Transform to rect local space
    let local_o = to_rect_local(origin, rect);
    let cos_r = rect.rotation.cos();
    let sin_r = rect.rotation.sin();
    let local_d = Vec2::new(dir.x * cos_r + dir.y * sin_r, -dir.x * sin_r + dir.y * cos_r);

    // AABB slab test
    let (mut tmin, mut tmax) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut normal_local = Vec2::ZERO;

    // X slab
    if local_d.x.abs() < 1e-10 {
        if local_o.x < -rect.half_extents.x || local_o.x > rect.half_extents.x {
            return None;
        }
    } else {
        let t1 = (-rect.half_extents.x - local_o.x) / local_d.x;
        let t2 = (rect.half_extents.x - local_o.x) / local_d.x;
        let (t_near, t_far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if t_near > tmin {
            tmin = t_near;
            normal_local = if local_d.x > 0.0 {
                Vec2::new(-1.0, 0.0)
            } else {
                Vec2::new(1.0, 0.0)
            };
        }
        tmax = tmax.min(t_far);
        if tmin > tmax {
            return None;
        }
    }

    // Y slab
    if local_d.y.abs() < 1e-10 {
        if local_o.y < -rect.half_extents.y || local_o.y > rect.half_extents.y {
            return None;
        }
    } else {
        let t1 = (-rect.half_extents.y - local_o.y) / local_d.y;
        let t2 = (rect.half_extents.y - local_o.y) / local_d.y;
        let (t_near, t_far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if t_near > tmin {
            tmin = t_near;
            normal_local = if local_d.y > 0.0 {
                Vec2::new(0.0, -1.0)
            } else {
                Vec2::new(0.0, 1.0)
            };
        }
        tmax = tmax.min(t_far);
        if tmin > tmax {
            return None;
        }
    }

    if tmin < 0.0 {
        return None;
    }

    let point = origin + dir * tmin;
    // Rotate normal back to world space
    let normal = Vec2::new(
        normal_local.x * cos_r - normal_local.y * sin_r,
        normal_local.x * sin_r + normal_local.y * cos_r,
    );

    Some(Hit {
        t: tmin,
        point,
        normal,
    })
}

/// Ray-triangle intersection.
pub fn ray_triangle(origin: Vec2, dir: Vec2, tri: &Triangle) -> Option<Hit> {
    let edges = [(tri.a, tri.b), (tri.b, tri.c), (tri.c, tri.a)];
    let mut best: Option<Hit> = None;

    for (start, end) in &edges {
        let edge_dir = *end - *start;
        let denom = dir.perp_dot(edge_dir);
        if denom.abs() < 1e-10 {
            continue;
        }
        let diff = *start - origin;
        let t = diff.perp_dot(edge_dir) / denom;
        let u = diff.perp_dot(dir) / denom;

        if t >= 0.0 && (0.0..=1.0).contains(&u) {
            if best.as_ref().is_none_or(|b| t < b.t) {
                let point = origin + dir * t;
                let normal = Vec2::new(-edge_dir.y, edge_dir.x).normalize();
                let normal = if normal.dot(dir) > 0.0 {
                    normal * -1.0
                } else {
                    normal
                };
                best = Some(Hit { t, point, normal });
            }
        }
    }

    best
}

/// Ray intersection against any geometry primitive.
pub fn ray_geometry(origin: Vec2, dir: Vec2, geom: &Geometry) -> Option<Hit> {
    match geom {
        Geometry::Circle(c) => ray_circle(origin, dir, c),
        Geometry::Segment(s) => ray_segment(origin, dir, s),
        Geometry::Rect(r) => ray_rect(origin, dir, r),
        Geometry::Triangle(t) => ray_triangle(origin, dir, t),
    }
}

// ---------------------------------------------------------------------------
// Separation force
// ---------------------------------------------------------------------------

/// Compute separation force direction and magnitude between two mobile entities
/// (circle-circle only for mobile-mobile). Returns (direction, magnitude) where
/// direction points from b toward a.
pub fn separation_force(a: &Circle, b: &Circle, max_force: f32) -> Option<(Vec2, f32)> {
    let diff = Vec2::new(a.center.x - b.center.x, a.center.y - b.center.y);
    let dist = diff.length();
    let min_dist = a.radius + b.radius;
    if dist >= min_dist {
        return None;
    }
    let penetration = min_dist - dist;
    let max_penetration = min_dist * 0.5; // cap at 50% overlap
    let magnitude = max_force * (penetration / max_penetration).min(1.0);
    let direction = if dist > 1e-6 {
        diff * (1.0 / dist)
    } else {
        Vec2::new(1.0, 0.0) // arbitrary direction for exact overlap
    };
    Some((direction, magnitude))
}

/// Compute the nearest point on a geometry primitive to a point, and the
/// penetration depth. Used for hard collision (mobile vs structure).
/// Returns (nearest_point, normal, penetration_depth) or None if no collision.
pub fn hard_collision(point: Vec2, radius: f32, geom: &Geometry) -> Option<(Vec2, Vec2, f32)> {
    match geom {
        Geometry::Circle(c) => {
            let diff = point - c.center;
            let dist = diff.length();
            let min_dist = radius + c.radius;
            if dist >= min_dist {
                return None;
            }
            let normal = if dist > 1e-6 {
                diff * (1.0 / dist)
            } else {
                Vec2::new(1.0, 0.0)
            };
            Some((c.center + normal * c.radius, normal, min_dist - dist))
        }
        Geometry::Segment(s) => {
            let dist = point_segment_distance(point, s.start, s.end);
            let effective_radius = radius + s.thickness / 2.0;
            if dist >= effective_radius {
                return None;
            }
            let nearest = nearest_point_on_segment(point, s.start, s.end);
            let diff = point - nearest;
            let d = diff.length();
            let normal = if d > 1e-6 {
                diff * (1.0 / d)
            } else {
                let seg_dir = s.end - s.start;
                Vec2::new(-seg_dir.y, seg_dir.x).normalize()
            };
            Some((nearest, normal, effective_radius - dist))
        }
        Geometry::Rect(r) => {
            let local = to_rect_local(point, r);
            let clamped = Vec2::new(
                local.x.clamp(-r.half_extents.x, r.half_extents.x),
                local.y.clamp(-r.half_extents.y, r.half_extents.y),
            );
            let diff = local - clamped;
            let dist = diff.length();
            if dist >= radius {
                return None;
            }
            let local_normal = if dist > 1e-6 {
                diff * (1.0 / dist)
            } else {
                // Point is inside the rect — push out along shortest axis
                let dx = r.half_extents.x - local.x.abs();
                let dy = r.half_extents.y - local.y.abs();
                if dx < dy {
                    Vec2::new(local.x.signum(), 0.0)
                } else {
                    Vec2::new(0.0, local.y.signum())
                }
            };
            let cos_r = r.rotation.cos();
            let sin_r = r.rotation.sin();
            let world_normal = Vec2::new(
                local_normal.x * cos_r - local_normal.y * sin_r,
                local_normal.x * sin_r + local_normal.y * cos_r,
            );
            let world_nearest = from_rect_local(clamped, r);
            Some((world_nearest, world_normal, radius - dist))
        }
        Geometry::Triangle(t) => {
            // Find nearest point on triangle boundary
            let edges = [(t.a, t.b), (t.b, t.c), (t.c, t.a)];
            let mut min_dist = f32::MAX;
            let mut nearest = t.a;
            for (s, e) in &edges {
                let np = nearest_point_on_segment(point, *s, *e);
                let d = (point - np).length();
                if d < min_dist {
                    min_dist = d;
                    nearest = np;
                }
            }
            // Also check if point is inside triangle
            if point_in_triangle(point, t) {
                let diff = point - nearest;
                let normal = if diff.length() > 1e-6 {
                    diff.normalize()
                } else {
                    Vec2::new(1.0, 0.0)
                };
                return Some((nearest, normal, radius + min_dist));
            }
            if min_dist >= radius {
                return None;
            }
            let diff = point - nearest;
            let normal = if diff.length() > 1e-6 {
                diff.normalize()
            } else {
                Vec2::new(1.0, 0.0)
            };
            Some((nearest, normal, radius - min_dist))
        }
    }
}

// ---------------------------------------------------------------------------
// Helper geometry functions
// ---------------------------------------------------------------------------

/// Distance from a point to a line segment.
fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let nearest = nearest_point_on_segment(p, a, b);
    (p - nearest).length()
}

/// Nearest point on a line segment to a given point.
fn nearest_point_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-10 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    Vec2::new(a.x + ab.x * t, a.y + ab.y * t)
}

/// Minimum distance between two line segments.
fn segment_segment_distance(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> f32 {
    // Check if segments intersect first
    if segments_intersect(a1, a2, b1, b2) {
        return 0.0;
    }
    // Otherwise, minimum of point-segment distances
    let d1 = point_segment_distance(a1, b1, b2);
    let d2 = point_segment_distance(a2, b1, b2);
    let d3 = point_segment_distance(b1, a1, a2);
    let d4 = point_segment_distance(b2, a1, a2);
    d1.min(d2).min(d3).min(d4)
}

/// Test if two line segments intersect.
fn segments_intersect(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> bool {
    let d1 = cross_2d(b2 - b1, a1 - b1);
    let d2 = cross_2d(b2 - b1, a2 - b1);
    let d3 = cross_2d(a2 - a1, b1 - a1);
    let d4 = cross_2d(a2 - a1, b2 - a1);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    // Collinear cases
    if d1.abs() < 1e-10 && on_segment(b1, b2, a1) {
        return true;
    }
    if d2.abs() < 1e-10 && on_segment(b1, b2, a2) {
        return true;
    }
    if d3.abs() < 1e-10 && on_segment(a1, a2, b1) {
        return true;
    }
    if d4.abs() < 1e-10 && on_segment(a1, a2, b2) {
        return true;
    }
    false
}

fn cross_2d(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn on_segment(p: Vec2, q: Vec2, r: Vec2) -> bool {
    r.x <= p.x.max(q.x) && r.x >= p.x.min(q.x) && r.y <= p.y.max(q.y) && r.y >= p.y.min(q.y)
}

/// Test if a point is inside a triangle using barycentric coordinates.
fn point_in_triangle(p: Vec2, t: &Triangle) -> bool {
    let d1 = sign(p, t.a, t.b);
    let d2 = sign(p, t.b, t.c);
    let d3 = sign(p, t.c, t.a);

    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);

    !(has_neg && has_pos)
}

fn sign(p1: Vec2, p2: Vec2, p3: Vec2) -> f32 {
    (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
}

/// Transform a world point into an oriented rect's local coordinate space.
fn to_rect_local(p: Vec2, r: &OrientedRect) -> Vec2 {
    let dx = p.x - r.center.x;
    let dy = p.y - r.center.y;
    let cos_r = r.rotation.cos();
    let sin_r = r.rotation.sin();
    Vec2::new(dx * cos_r + dy * sin_r, -dx * sin_r + dy * cos_r)
}

/// Transform a local-space point back to world space.
fn from_rect_local(p: Vec2, r: &OrientedRect) -> Vec2 {
    let cos_r = r.rotation.cos();
    let sin_r = r.rotation.sin();
    Vec2::new(
        p.x * cos_r - p.y * sin_r + r.center.x,
        p.x * sin_r + p.y * cos_r + r.center.y,
    )
}

/// Get the two separating axes of an oriented rect.
fn rect_axes(r: &OrientedRect) -> [Vec2; 2] {
    let cos_r = r.rotation.cos();
    let sin_r = r.rotation.sin();
    [
        Vec2::new(cos_r, sin_r),
        Vec2::new(-sin_r, cos_r),
    ]
}

/// Get the four corners of an oriented rect in world space.
fn rect_corners(r: &OrientedRect) -> [Vec2; 4] {
    let cos_r = r.rotation.cos();
    let sin_r = r.rotation.sin();
    let hx = r.half_extents.x;
    let hy = r.half_extents.y;

    let ax = Vec2::new(cos_r * hx, sin_r * hx);
    let ay = Vec2::new(-sin_r * hy, cos_r * hy);

    [
        Vec2::new(r.center.x + ax.x + ay.x, r.center.y + ax.y + ay.y),
        Vec2::new(r.center.x - ax.x + ay.x, r.center.y - ax.y + ay.y),
        Vec2::new(r.center.x - ax.x - ay.x, r.center.y - ax.y - ay.y),
        Vec2::new(r.center.x + ax.x - ay.x, r.center.y + ax.y - ay.y),
    ]
}

/// Project corners onto an axis and return (min, max).
fn project_corners(corners: &[Vec2], axis: Vec2) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for c in corners {
        let proj = c.dot(axis);
        min = min.min(proj);
        max = max.max(proj);
    }
    (min, max)
}

/// Test a segment (in local space) against an AABB centered at origin with given half-extents.
fn segment_aabb(start: Vec2, end: Vec2, half: Vec2) -> bool {
    // Cohen-Sutherland style — if any point is inside, it intersects
    if start.x.abs() <= half.x && start.y.abs() <= half.y {
        return true;
    }
    if end.x.abs() <= half.x && end.y.abs() <= half.y {
        return true;
    }
    // Test segment against each edge of the AABB
    let aabb_edges = [
        (
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
        ),
        (
            Vec2::new(half.x, -half.y),
            Vec2::new(half.x, half.y),
        ),
        (
            Vec2::new(half.x, half.y),
            Vec2::new(-half.x, half.y),
        ),
        (
            Vec2::new(-half.x, half.y),
            Vec2::new(-half.x, -half.y),
        ),
    ];
    for (s, e) in &aabb_edges {
        if segments_intersect(start, end, *s, *e) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_circle_overlap() {
        let a = Circle {
            center: Vec2::ZERO,
            radius: 5.0,
        };
        let b = Circle {
            center: Vec2::new(8.0, 0.0),
            radius: 5.0,
        };
        assert!(circle_circle(&a, &b));

        let c = Circle {
            center: Vec2::new(20.0, 0.0),
            radius: 5.0,
        };
        assert!(!circle_circle(&a, &c));
    }

    #[test]
    fn circle_segment_overlap() {
        let c = Circle {
            center: Vec2::ZERO,
            radius: 5.0,
        };
        let s = LineSegment {
            start: Vec2::new(4.0, -10.0),
            end: Vec2::new(4.0, 10.0),
            thickness: 1.0,
        };
        assert!(circle_segment(&c, &s));

        let s_far = LineSegment {
            start: Vec2::new(20.0, -10.0),
            end: Vec2::new(20.0, 10.0),
            thickness: 1.0,
        };
        assert!(!circle_segment(&c, &s_far));
    }

    #[test]
    fn circle_rect_overlap() {
        let c = Circle {
            center: Vec2::ZERO,
            radius: 5.0,
        };
        let r = OrientedRect {
            center: Vec2::new(6.0, 0.0),
            half_extents: Vec2::new(3.0, 3.0),
            rotation: 0.0,
        };
        assert!(circle_rect(&c, &r));
    }

    #[test]
    fn circle_triangle_overlap() {
        let c = Circle {
            center: Vec2::ZERO,
            radius: 2.0,
        };
        let t = Triangle {
            a: Vec2::new(1.0, 0.0),
            b: Vec2::new(5.0, -3.0),
            c: Vec2::new(5.0, 3.0),
        };
        assert!(circle_triangle(&c, &t));
    }

    #[test]
    fn rect_rect_overlap() {
        let a = OrientedRect {
            center: Vec2::ZERO,
            half_extents: Vec2::new(5.0, 5.0),
            rotation: 0.0,
        };
        let b = OrientedRect {
            center: Vec2::new(8.0, 0.0),
            half_extents: Vec2::new(5.0, 5.0),
            rotation: 0.0,
        };
        assert!(rect_rect(&a, &b));

        let c = OrientedRect {
            center: Vec2::new(20.0, 0.0),
            half_extents: Vec2::new(5.0, 5.0),
            rotation: 0.0,
        };
        assert!(!rect_rect(&a, &c));
    }

    #[test]
    fn triangle_triangle_overlap() {
        let a = Triangle {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(4.0, 0.0),
            c: Vec2::new(2.0, 4.0),
        };
        let b = Triangle {
            a: Vec2::new(2.0, 0.0),
            b: Vec2::new(6.0, 0.0),
            c: Vec2::new(4.0, 4.0),
        };
        assert!(triangle_triangle(&a, &b));
    }

    #[test]
    fn ray_circle_hit() {
        let c = Circle {
            center: Vec2::new(10.0, 0.0),
            radius: 3.0,
        };
        let hit = ray_circle(Vec2::ZERO, Vec2::new(1.0, 0.0), &c);
        assert!(hit.is_some());
        let h = hit.unwrap();
        assert!((h.t - 7.0).abs() < 0.1);
    }

    #[test]
    fn ray_circle_miss() {
        let c = Circle {
            center: Vec2::new(10.0, 10.0),
            radius: 1.0,
        };
        let hit = ray_circle(Vec2::ZERO, Vec2::new(1.0, 0.0), &c);
        assert!(hit.is_none());
    }

    #[test]
    fn separation_force_overlapping() {
        let a = Circle {
            center: Vec2::ZERO,
            radius: 5.0,
        };
        let b = Circle {
            center: Vec2::new(8.0, 0.0),
            radius: 5.0,
        };
        let result = separation_force(&a, &b, 100.0);
        assert!(result.is_some());
        let (dir, mag) = result.unwrap();
        assert!(dir.x < 0.0); // b pushes a to the left (wait, direction is from b toward a)
        // Actually: diff = a - b = (-8, 0), normalized = (-1, 0)
        assert!(dir.x < 0.0);
        assert!(mag > 0.0);
    }

    #[test]
    fn separation_force_no_overlap() {
        let a = Circle {
            center: Vec2::ZERO,
            radius: 3.0,
        };
        let b = Circle {
            center: Vec2::new(20.0, 0.0),
            radius: 3.0,
        };
        assert!(separation_force(&a, &b, 100.0).is_none());
    }

    #[test]
    fn hard_collision_circle() {
        let geom = Geometry::Circle(Circle {
            center: Vec2::new(10.0, 0.0),
            radius: 3.0,
        });
        let result = hard_collision(Vec2::new(8.0, 0.0), 2.0, &geom);
        assert!(result.is_some());
    }

    #[test]
    fn intersects_dispatch() {
        let a = Geometry::Circle(Circle {
            center: Vec2::ZERO,
            radius: 5.0,
        });
        let b = Geometry::Segment(LineSegment {
            start: Vec2::new(4.0, -10.0),
            end: Vec2::new(4.0, 10.0),
            thickness: 1.0,
        });
        assert!(intersects(&a, &b));
    }

    #[test]
    fn point_in_triangle_inside() {
        let t = Triangle {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(10.0, 0.0),
            c: Vec2::new(5.0, 10.0),
        };
        assert!(point_in_triangle(Vec2::new(5.0, 3.0), &t));
        assert!(!point_in_triangle(Vec2::new(20.0, 20.0), &t));
    }
}
