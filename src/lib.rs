#![no_std]

// SAFETY POLICY: Single-threaded WASM execution only
// This code uses UnsafeCell without synchronization.
// DO NOT compile with WASM atomics/threads without adding Mutex protection.
#[cfg(target_feature = "atomics")]
compile_error!("This code is NOT thread-safe. Add Mutex<Editor> before enabling atomics.");

extern crate alloc;

mod graph;
mod debug_checks;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::f32::consts::PI;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::hint::spin_loop;
use graph::GraphStore;
use debug_checks::*;

const HEAP_SIZE: usize = 1024 * 1024;

struct BumpAllocator {
    offset: AtomicUsize,
}

struct Heap {
    buf: UnsafeCell<[u8; HEAP_SIZE]>,
}

unsafe impl Sync for Heap {}

static HEAP: Heap = Heap {
    buf: UnsafeCell::new([0; HEAP_SIZE]),
};

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let mut start = self.offset.load(Ordering::Relaxed);

        loop {
            let aligned = (start + align - 1) & !(align - 1);
            let end = aligned.saturating_add(size);
            if end > HEAP_SIZE {
                loop {
                    spin_loop();
                }
            }

            match self.offset.compare_exchange(
                start,
                end,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return HEAP.buf.get().cast::<u8>().add(aligned),
                Err(next) => start = next,
            }
        }
    }

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

#[global_allocator]
static GLOBAL: BumpAllocator = BumpAllocator {
    offset: AtomicUsize::new(0),
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        spin_loop();
    }
}

// -------- Editor core --------
const TOLERANCE: f32 = 10.0;
const SNAP_EPS: f32 = 0.25; // Grid snap for intersection deduplication
#[allow(dead_code)]
const MAX_TRACE_STEPS: u32 = 2048;
const GAP_RADIUS: f32 = 12.0;
const MIN_AREA: f32 = 50.0;
const FRAC_3_PI_4: f32 = PI * 0.75;

fn floor_f32(x: f32) -> f32 {
    let truncated = x as i32 as f32;
    if truncated <= x {
        truncated
    } else {
        truncated - 1.0
    }
}

#[allow(dead_code)]
fn snap_coord(v: f32) -> f32 {
    floor_f32(v / SNAP_EPS) * SNAP_EPS
}

fn key_from_point(p: Point) -> (i32, i32) {
    let kx = floor_f32(p.x / SNAP_EPS) as i32;
    let ky = floor_f32(p.y / SNAP_EPS) as i32;
    (kx, ky)
}

#[allow(dead_code)]
fn find_node_index(nodes: &[Point], p: Point) -> Option<u32> {
    let key = key_from_point(p);
    for (i, node) in nodes.iter().enumerate() {
        let node_key = key_from_point(*node);
        if node_key == key {
            return Some(i as u32);
        }
    }
    None
}

fn parse_hex_color(hex: &[u8]) -> u32 {
    let hex_str = core::str::from_utf8(hex).unwrap_or("747474");
    let start = if hex_str.starts_with('#') { 1 } else { 0 };
    let hex_trimmed = &hex_str[start..];
    let color = u32::from_str_radix(hex_trimmed, 16).unwrap_or(0x747474);
    (color << 8) | 0xFF
}

#[derive(Clone, Copy)]
struct Line {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
}

impl Point {
    #[allow(dead_code)]
    fn dist_sq(&self, other: Point) -> f32 {
        distance_sq(self.x, self.y, other.x, other.y)
    }
}

#[derive(Clone, Copy)]
struct Seg {
    a: u32,
    b: u32,
}

#[derive(Clone, Copy)]
struct HalfEdge {
    from: u32,
    to: u32,
    #[allow(dead_code)]
    seg: u32,
}

struct IntersectionRegistry {
    entries: Vec<(i32, i32, u32)>, // (qx, qy, node_id)
}

impl IntersectionRegistry {
    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn quantize(x: f32, y: f32) -> (i32, i32) {
        let qx = round_to_i32(x * 1024.0);
        let qy = round_to_i32(y * 1024.0);
        (qx, qy)
    }

    fn get_or_insert(&mut self, x: f32, y: f32, nodes: &mut Vec<Point>) -> u32 {
        let (qx, qy) = Self::quantize(x, y);
        for (ix, (kq_x, kq_y, nid)) in self.entries.iter().enumerate() {
            if *kq_x == qx && *kq_y == qy {
                // safety: ensure nodes len matches stored id
                let _ = ix;
                return *nid;
            }
        }
        let new_id = nodes.len() as u32;
        nodes.push(Point { x, y });
        self.entries.push((qx, qy, new_id));
        new_id
    }
}

#[allow(dead_code)]
fn other_end(seg: &Seg, node: u32) -> u32 {
    if seg.a == node { seg.b } else { seg.a }
}

#[derive(Clone)]
struct FillGraph {
    nodes: Vec<Point>,
    segments: Vec<Seg>,
    half_edges: Vec<HalfEdge>,
    outgoing: Vec<Vec<u32>>, // node -> list of half_edge indices
    node_sectors: Vec<Vec<u32>>, // node -> list of segment indices (undirected)
}

#[derive(Clone)]
struct Polygon {
    points: Vec<(f32, f32)>,
    color: u32,
}

impl Polygon {
    fn new() -> Self {
        Self {
            points: Vec::new(),
            color: 0x747474FF,
        }
    }

    fn push(&mut self, x: f32, y: f32) {
        self.points.push((x, y));
    }

    fn is_closed(&self) -> bool {
        self.points.len() > 2
    }

    fn with_color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }
}

enum Command {
    Add,
    AddFill,
    AddFrame, // Grouped undo for 4 frame lines
    Clear(Vec<Line>, Vec<Polygon>),
    CleanOverhangs(Vec<Line>), // Save previous lines before cleanup
}

fn distance_sq(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    dx * dx + dy * dy
}

#[inline(always)]
fn absf(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}

fn round_to_i32(v: f32) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

#[inline(always)]
fn cross(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    ax * by - ay * bx
}

#[allow(dead_code)]
fn dot(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    ax * bx + ay * by
}

#[allow(dead_code)]
fn norm(ax: f32, ay: f32) -> f32 {
    sqrt_approx(ax * ax + ay * ay)
}

// Project point P onto line segment AB
// Returns (t, qx, qy, dist2)
fn point_segment_nearest(
    px: f32, py: f32,
    ax: f32, ay: f32,
    bx: f32, by: f32,
) -> (f32, f32, f32, f32) {
    let abx = bx - ax;
    let aby = by - ay;
    let ab2 = abx * abx + aby * aby;

    if ab2 < 0.0001 {
        let dist2 = distance_sq(px, py, ax, ay);
        return (0.0, ax, ay, dist2);
    }

    let apx = px - ax;
    let apy = py - ay;
    let t = (apx * abx + apy * aby) / ab2;
    let t_clamped = if t < 0.0 { 0.0 } else if t > 1.0 { 1.0 } else { t };

    let qx = ax + t_clamped * abx;
    let qy = ay + t_clamped * aby;
    let dist2 = distance_sq(px, py, qx, qy);

    (t_clamped, qx, qy, dist2)
}

// Compute intersection of two line segments
// Returns Some((t1, t2, ix, iy)) if they intersect within [0,1] on both
#[allow(dead_code)]
fn line_intersection_params(
    x1: f32, y1: f32, x2: f32, y2: f32,
    x3: f32, y3: f32, x4: f32, y4: f32,
) -> Option<(f32, f32, f32, f32)> {
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if absf(denom) < 0.0001 {
        return None;
    }

    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
    let u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / denom;

    if t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0 {
        let ix = x1 + t * (x2 - x1);
        let iy = y1 + t * (y2 - y1);
        return Some((t, u, ix, iy));
    }

    None
}

fn find_nearest_line(lines: &[Line], x: f32, y: f32, max_dist_sq: f32) -> Option<usize> {
    let mut best_idx = None;
    let mut best_dist_sq = max_dist_sq;

    for (i, line) in lines.iter().enumerate() {
        let dist_sq_start = distance_sq(x, y, line.x1, line.y1);
        let dist_sq_end = distance_sq(x, y, line.x2, line.y2);

        if dist_sq_start < best_dist_sq {
            best_dist_sq = dist_sq_start;
            best_idx = Some(i);
        }

        if dist_sq_end < best_dist_sq {
            best_dist_sq = dist_sq_end;
            best_idx = Some(i);
        }
    }

    best_idx
}

fn find_connected_lines(lines: &[Line], start_idx: usize) -> Vec<usize> {
    if lines.is_empty() || start_idx >= lines.len() {
        return Vec::new();
    }

    let mut connected = Vec::new();
    let mut visited = Vec::new();
    for _ in 0..lines.len() {
        visited.push(false);
    }
    let mut queue = Vec::new();

    queue.push(start_idx);
    visited[start_idx] = true;

    while !queue.is_empty() {
        if let Some(idx) = queue.pop() {
            connected.push(idx);
            let current_line = &lines[idx];

            for (other_idx, other_line) in lines.iter().enumerate() {
                if visited[other_idx] {
                    continue;
                }

                let connects = 
                    (absf(current_line.x1 - other_line.x1) + absf(current_line.y1 - other_line.y1) < TOLERANCE) ||
                    (absf(current_line.x1 - other_line.x2) + absf(current_line.y1 - other_line.y2) < TOLERANCE) ||
                    (absf(current_line.x2 - other_line.x1) + absf(current_line.y2 - other_line.y1) < TOLERANCE) ||
                    (absf(current_line.x2 - other_line.x2) + absf(current_line.y2 - other_line.y2) < TOLERANCE);

                if connects {
                    visited[other_idx] = true;
                    queue.push(other_idx);
                }
            }
        }
    }

    connected
}

fn trace_polygon(lines: &[Line], line_indices: &[usize]) -> Polygon {
    let mut polygon = Polygon::new();

    if line_indices.is_empty() {
        return polygon;
    }

    let mut used = Vec::new();
    for _ in 0..line_indices.len() {
        used.push(false);
    }

    let mut current_end = (lines[line_indices[0]].x2, lines[line_indices[0]].y2);

    polygon.push(lines[line_indices[0]].x1, lines[line_indices[0]].y1);
    polygon.push(current_end.0, current_end.1);
    used[0] = true;

    for _ in 1..line_indices.len() {
        let mut found = false;
        for i in 0..line_indices.len() {
            if used[i] {
                continue;
            }
            let idx = line_indices[i];
            let line = &lines[idx];

            let dx1 = absf(current_end.0 - line.x1);
            let dy1 = absf(current_end.1 - line.y1);
            let dx2 = absf(current_end.0 - line.x2);
            let dy2 = absf(current_end.1 - line.y2);

            if dx1 + dy1 < 1.0 {
                polygon.push(line.x2, line.y2);
                current_end = (line.x2, line.y2);
                used[i] = true;
                found = true;
                break;
            } else if dx2 + dy2 < 1.0 {
                polygon.push(line.x1, line.y1);
                current_end = (line.x1, line.y1);
                used[i] = true;
                found = true;
                break;
            }
        }
        if !found {
            break;
        }
    }

    polygon
}

struct Editor {
    lines: Vec<Line>,
    fills: Vec<Polygon>,
    history: Vec<Command>,
    export_buf: Vec<f32>,
    debug_buf: Vec<f32>,
    debug_enabled: bool,
    intersections: Vec<(f32, f32)>,
    intersections_export: Vec<f32>,
    fill_graph: FillGraph,
    fill_trace_buf: Vec<f32>,
    fills_export_buf: Vec<f32>,
    fill_candidates_buf: Vec<f32>,  // Logs candidate segments at each junction
    adjacency_debug_buf: Vec<f32>,  // Dumps all outgoing edges at junction for debugging
    node_outgoing_buf: Vec<f32>,    // Outgoing half-edges for current node
    node_audit_buf: Vec<f32>,       // Duplicate node key audit
    graph_debug_buf: Vec<f32>,      // Export of cut segment graph for visualization
    node_degree: Vec<u32>,          // Degree per node
    allow_node: Vec<bool>,          // Leaf-stripped allowed nodes
    effective_degree: Vec<u32>,     // Effective degree after pruning
    fill_walk_debug_buf: Vec<f32>,  // Walk step debugging
    fill_color: u32,                // Current fill color (RGBA)
    graph_store: GraphStore,        // Incremental closed-component tracker
}

// Compute a simple angle proxy for sorting (0-4 range for quadrants)
fn compute_angle_proxy(dx: f32, dy: f32) -> f32 {
    if absf(dx) < 0.0001 && absf(dy) < 0.0001 {
        return 0.0;
    }
    
    let abs_ratio = if absf(dx) > 0.0001 { absf(dy / dx) } else { 1000.0 };
    
    if dx > 0.0 && dy >= 0.0 {
        // Quadrant 1: 0-1
        abs_ratio
    } else if dx <= 0.0 && dy > 0.0 {
        // Quadrant 2: 1-2
        2.0 - abs_ratio
    } else if dx < 0.0 && dy <= 0.0 {
        // Quadrant 3: 2-3
        2.0 + abs_ratio
    } else {
        // Quadrant 4: 3-4
        4.0 - abs_ratio
    }
}

fn normalize_with_len(dx: f32, dy: f32) -> (f32, f32, f32) {
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        return (0.0, 0.0, 0.0);
    }
    let len = sqrt_approx(len_sq);
    let inv_len = if len > 0.0 { 1.0 / len } else { 0.0 };
    (dx * inv_len, dy * inv_len, len)
}

#[allow(dead_code)]
fn normalize(dx: f32, dy: f32) -> (f32, f32) {
    let length = norm(dx, dy);
    if length < 1e-12 {
        (0.0, 0.0)
    } else {
        let inv = 1.0 / length;
        (dx * inv, dy * inv)
    }
}

// Lightweight sqrt approximation (Newton-Raphson); good enough for ordering
fn sqrt_approx(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut guess = if x > 1.0 { x } else { 1.0 };
    // A few iterations are sufficient for monotonicity and decent accuracy
    for _ in 0..4 {
        guess = 0.5 * (guess + x / guess);
    }
    guess
}

// Fast atan2 approximation suitable for ordering angles; deterministic
fn atan2_approx(y: f32, x: f32) -> f32 {
    if x == 0.0 {
        if y > 0.0 {
            return core::f32::consts::FRAC_PI_2;
        } else if y < 0.0 {
            return -core::f32::consts::FRAC_PI_2;
        } else {
            return 0.0;
        }
    }

    let abs_y = if y < 0.0 { -y } else { y } + 1e-12; // prevent div by zero
    let mut angle = if x > 0.0 {
        let r = (x - abs_y) / (x + abs_y);
        core::f32::consts::FRAC_PI_4 - core::f32::consts::FRAC_PI_4 * r
    } else {
        let r = (x + abs_y) / (abs_y - x);
        FRAC_3_PI_4 - core::f32::consts::FRAC_PI_4 * r
    };

    if y < 0.0 {
        angle = -angle;
    }
    angle
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum FailReason {
    DeadEnd,
    StepLimit,
    PrematureCycle,
    NoClosure,
}

#[derive(Clone, Copy, Debug)]
enum SideRule {
    KeepLeft,
    KeepRight,
}

#[derive(Clone)]
struct StepCandidateDebug {
    next: u32,
    turn: f32,
    abs_turn: f32,
    out_x: f32,
    out_y: f32,
    is_virtual: bool,
    accepted_side: bool,
    was_visited: bool,
    chosen: bool,
    len: f32,
    mid_dist: f32,
}

#[derive(Clone)]
struct StepDebug {
    step_idx: u32,
    cur: u32,
    prev: u32,
    vin: (f32, f32),
    candidates: Vec<StepCandidateDebug>,
}

struct TraceResult {
    closed: bool,
    points: Vec<(f32, f32)>,
    #[allow(dead_code)]
    fail_reason: Option<FailReason>,
    #[allow(dead_code)]
    steps: u32,
    step_debug: Vec<StepDebug>,
}

impl TraceResult {
    fn success(points: Vec<(f32, f32)>, steps: u32, step_debug: Vec<StepDebug>) -> Self {
        Self {
            closed: true,
            points,
            fail_reason: None,
            steps,
            step_debug,
        }
    }

    fn fail(reason: FailReason, steps: u32, step_debug: Vec<StepDebug>) -> Self {
        Self {
            closed: false,
            points: Vec::new(),
            fail_reason: Some(reason),
            steps,
            step_debug,
        }
    }
}

// Compute signed area of polygon (2x area)
// Positive = CCW winding, Negative = CW
fn signed_area_2x(points: &[(f32, f32)]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..points.len() {
        let p0 = points[i];
        let p1 = points[(i + 1) % points.len()];
        area += p0.0 * p1.1 - p1.0 * p0.1;
    }
    area
}

// Point in polygon using even-odd rule with robustness against vertex hits
fn point_in_poly_evenodd(pt: (f32, f32), poly: &[(f32, f32)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    const HIT_SLOP: f32 = 1e-6;
    let (px, py) = pt;
    let mut crossings = 0;
    let mut j = poly.len() - 1;
    
    for i in 0..poly.len() {
        let (x0, y0) = poly[j];
        let (x1, y1) = poly[i];
        
        // Check if point is very close to vertex
        if absf(x1 - px) < HIT_SLOP && absf(y1 - py) < HIT_SLOP {
            return false; // On boundary
        }
        
        // Standard even-odd: check if horizontal ray hits this edge
        // Only count if edge crosses the ray (not just touches)
        if (y0 > py) != (y1 > py) {
            // Edge crosses horizontal line at py
            let x_intersect = (x1 - x0) * (py - y0) / (y1 - y0) + x0;
            if px < x_intersect {
                crossings += 1;
            }
        }
        j = i;
    }
    crossings % 2 == 1
}

// Distance from point to polygon boundary (squared)
fn min_dist_sq_to_polygon(pt: (f32, f32), poly: &[(f32, f32)]) -> f32 {
    if poly.len() < 2 {
        return f32::INFINITY;
    }
    let (px, py) = pt;
    let mut min_dist2 = f32::INFINITY;
    
    for i in 0..poly.len() {
        let p0 = poly[i];
        let p1 = poly[(i + 1) % poly.len()];
        
        // Distance from point to line segment
        let dx = p1.0 - p0.0;
        let dy = p1.1 - p0.1;
        let len2 = dx * dx + dy * dy;
        
        if len2 < 1e-12 {
            let d2 = (px - p0.0) * (px - p0.0) + (py - p0.1) * (py - p0.1);
            min_dist2 = if d2 < min_dist2 { d2 } else { min_dist2 };
            continue;
        }
        
        let t = ((px - p0.0) * dx + (py - p0.1) * dy) / len2;
        let t_clamped = if t < 0.0 { 0.0 } else if t > 1.0 { 1.0 } else { t };
        
        let cx = p0.0 + t_clamped * dx;
        let cy = p0.1 + t_clamped * dy;
        let d2 = (px - cx) * (px - cx) + (py - cy) * (py - cy);
        
        min_dist2 = if d2 < min_dist2 { d2 } else { min_dist2 };
    }
    min_dist2
}

// Check if polygon is simple (non-self-intersecting) - quick check
fn is_simple_polygon(poly: &[(f32, f32)]) -> bool {
    if poly.len() < 4 {
        return true;
    }
    // Quick check: if first == last, it's properly closed
    poly.first() == poly.last()
}

// Get polygon bounding box
fn poly_bounds(poly: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    if poly.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut minx = poly[0].0;
    let mut maxx = poly[0].0;
    let mut miny = poly[0].1;
    let mut maxy = poly[0].1;
    
    for pt in poly.iter().skip(1) {
        if pt.0 < minx { minx = pt.0; }
        if pt.0 > maxx { maxx = pt.0; }
        if pt.1 < miny { miny = pt.1; }
        if pt.1 > maxy { maxy = pt.1; }
    }
    (minx, miny, maxx, maxy)
}

impl Editor {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            fills: Vec::new(),
            history: Vec::new(),
            export_buf: Vec::new(),
            debug_buf: Vec::new(),
            debug_enabled: false,
            intersections: Vec::new(),
            intersections_export: Vec::new(),
            fill_graph: FillGraph {
                nodes: Vec::new(),
                segments: Vec::new(),
                half_edges: Vec::new(),
                outgoing: Vec::new(),
                node_sectors: Vec::new(),
            },
            fill_trace_buf: Vec::new(),
            fills_export_buf: Vec::new(),
            fill_candidates_buf: Vec::new(),
            adjacency_debug_buf: Vec::new(),
            node_outgoing_buf: Vec::new(),
            node_audit_buf: Vec::new(),
            graph_debug_buf: Vec::new(),
            node_degree: Vec::new(),
            allow_node: Vec::new(),
            effective_degree: Vec::new(),
            fill_walk_debug_buf: Vec::new(),
            fill_color: 0x747474FF,
            graph_store: GraphStore::new(),
        }
    }

    fn build_fill_graph(&mut self) {
        self.fill_graph.nodes.clear();
        self.fill_graph.segments.clear();
            self.fill_graph.half_edges.clear();
            self.fill_graph.outgoing.clear();
            self.fill_graph.node_sectors.clear();
        self.fill_graph.node_sectors.clear();
        self.intersections.clear();
        self.intersections_export.clear();

        if self.lines.is_empty() {
            return;
        }

        // Build graph_store from all lines to track closed components
        // This is used ONLY for fill filtering, not for debug/graph visualization
        self.graph_store.clear();
        for (idx, line) in self.lines.iter().enumerate() {
            self.graph_store.add_segment(line.x1, line.y1, line.x2, line.y2, idx);
        }

        // For debug and graph visualization, we build the FULL graph (all lines)
        // Closed component filtering is only applied during actual fill operation
        
        let mut registry = IntersectionRegistry::new();
        let mut per_line_nodes: Vec<Vec<(f32, u32)>> = Vec::new();
        per_line_nodes.resize(self.lines.len(), Vec::new());

        // Insert endpoints for ALL lines (for debug/graph visualization)
        for (idx, line) in self.lines.iter().enumerate() {
            let n0 = registry.get_or_insert(line.x1, line.y1, &mut self.fill_graph.nodes);
            let n1 = registry.get_or_insert(line.x2, line.y2, &mut self.fill_graph.nodes);
            per_line_nodes[idx].push((0.0, n0));
            per_line_nodes[idx].push((1.0, n1));
        }

        // Collect intersections with shared node ids (ALL lines)
        let mut intersection_node_ids: Vec<u32> = Vec::new();
        for i in 0..self.lines.len() {
            for j in (i + 1)..self.lines.len() {
                let l1 = &self.lines[i];
                let l2 = &self.lines[j];
                if let Some((t1, t2, ix, iy)) = line_intersection_params(
                    l1.x1, l1.y1, l1.x2, l1.y2,
                    l2.x1, l2.y1, l2.x2, l2.y2,
                ) {
                    if t1 > 0.0 && t1 < 1.0 && t2 > 0.0 && t2 < 1.0 {
                        let node_id = registry.get_or_insert(ix, iy, &mut self.fill_graph.nodes);
                        per_line_nodes[i].push((t1, node_id));
                        per_line_nodes[j].push((t2, node_id));

                        // Track intersection nodes for export (dedup by node id)
                        let mut already = false;
                        for existing in intersection_node_ids.iter() {
                            if *existing == node_id {
                                already = true;
                                break;
                            }
                        }
                        if !already {
                            intersection_node_ids.push(node_id);
                        }
                    }
                }
            }
        }

        // Build segments along each line using node ids
        for nodes_on_line in per_line_nodes.iter_mut() {
            if nodes_on_line.len() < 2 {
                continue;
            }

            nodes_on_line.sort_by(|a, b| {
                if a.0 < b.0 { core::cmp::Ordering::Less }
                else if a.0 > b.0 { core::cmp::Ordering::Greater }
                else { core::cmp::Ordering::Equal }
            });

            // Deduplicate consecutive identical node ids
            let mut compact: Vec<(f32, u32)> = Vec::new();
            for (t, nid) in nodes_on_line.iter() {
                if compact.is_empty() || compact[compact.len() - 1].1 != *nid {
                    compact.push((*t, *nid));
                }
            }

            if compact.len() < 2 {
                continue;
            }

            for k in 0..compact.len() - 1 {
                let a = compact[k].1;
                let b = compact[k + 1].1;
                if a != b {
                    self.fill_graph.segments.push(Seg { a, b });
                }
            }
        }

        // Export intersections for UI/debug
        // Sort intersection ids for deterministic export
        if intersection_node_ids.len() > 1 {
            intersection_node_ids.sort();
            intersection_node_ids.dedup();
        }

        self.intersections_export.clear();
        self.intersections_export.push(intersection_node_ids.len() as f32);
        for nid in intersection_node_ids.iter() {
            if (*nid as usize) < self.fill_graph.nodes.len() {
                let p = self.fill_graph.nodes[*nid as usize];
                self.intersections_export.push(p.x);
                self.intersections_export.push(p.y);
            }
        }

        // Build half-edges
        for (seg_idx, seg) in self.fill_graph.segments.iter().enumerate() {
            let he1 = HalfEdge {
                from: seg.a,
                to: seg.b,
                seg: seg_idx as u32,
            };
            let he2 = HalfEdge {
                from: seg.b,
                to: seg.a,
                seg: seg_idx as u32,
            };
            self.fill_graph.half_edges.push(he1);
            self.fill_graph.half_edges.push(he2);
        }

        // Build undirected adjacency for sectors
        self.fill_graph.node_sectors.clear();
        for _ in 0..self.fill_graph.nodes.len() {
            self.fill_graph.node_sectors.push(Vec::new());
        }
        for (seg_idx, seg) in self.fill_graph.segments.iter().enumerate() {
            if (seg.a as usize) < self.fill_graph.node_sectors.len() {
                self.fill_graph.node_sectors[seg.a as usize].push(seg_idx as u32);
            }
            if (seg.b as usize) < self.fill_graph.node_sectors.len() {
                self.fill_graph.node_sectors[seg.b as usize].push(seg_idx as u32);
            }
        }

        // Build outgoing lists and sort by angle
        self.fill_graph.outgoing.clear();
        for _ in 0..self.fill_graph.nodes.len() {
            self.fill_graph.outgoing.push(Vec::new());
        }

        for (he_idx, he) in self.fill_graph.half_edges.iter().enumerate() {
            self.fill_graph.outgoing[he.from as usize].push(he_idx as u32);
        }

        // Sort each outgoing list by angle from node (using atan2-like logic)
        for node_idx in 0..self.fill_graph.nodes.len() {
            let node = self.fill_graph.nodes[node_idx];
            let outgoing_edges = &mut self.fill_graph.outgoing[node_idx];

            outgoing_edges.sort_by(|&he_idx1, &he_idx2| {
                let he1 = self.fill_graph.half_edges[he_idx1 as usize];
                let he2 = self.fill_graph.half_edges[he_idx2 as usize];
                let to1 = self.fill_graph.nodes[he1.to as usize];
                let to2 = self.fill_graph.nodes[he2.to as usize];

                let dx1 = to1.x - node.x;
                let dy1 = to1.y - node.y;
                let dx2 = to2.x - node.x;
                let dy2 = to2.y - node.y;

                // Compute a simple angle proxy: quadrant + dy/dx ratio
                let angle1 = compute_angle_proxy(dx1, dy1);
                let angle2 = compute_angle_proxy(dx2, dy2);

                if angle1 < angle2 { core::cmp::Ordering::Less }
                else if angle1 > angle2 { core::cmp::Ordering::Greater }
                else { core::cmp::Ordering::Equal }
            });
        }

        // Node audit for duplicate snapped keys
        self.node_audit_buf.clear();
        if !self.fill_graph.nodes.is_empty() {
            let mut keyed: Vec<(i32, i32, u32, f32, f32)> = Vec::new();
            for (idx, p) in self.fill_graph.nodes.iter().enumerate() {
                let k = key_from_point(*p);
                keyed.push((k.0, k.1, idx as u32, p.x, p.y));
            }
            keyed.sort_by(|a, b| {
                if a.0 == b.0 {
                    if a.1 == b.1 {
                        a.2.cmp(&b.2)
                    } else if a.1 < b.1 { core::cmp::Ordering::Less } else { core::cmp::Ordering::Greater }
                } else if a.0 < b.0 { core::cmp::Ordering::Less } else { core::cmp::Ordering::Greater }
            });

            let mut group_count = 0u32;
            // Reserve first slot for group_count; fill later
            self.node_audit_buf.push(0.0);

            let mut i = 0usize;
            while i < keyed.len() {
                let (kx, ky, _id, _x, _y) = keyed[i];
                let mut j = i + 1;
                while j < keyed.len() && keyed[j].0 == kx && keyed[j].1 == ky {
                    j += 1;
                }
                let dup_len = j - i;
                if dup_len > 1 {
                    group_count += 1;
                    self.node_audit_buf.push(kx as f32);
                    self.node_audit_buf.push(ky as f32);
                    self.node_audit_buf.push(dup_len as f32);
                    for k in i..j {
                        let (_kx2, _ky2, nid, nx, ny) = keyed[k];
                        self.node_audit_buf.push(nid as f32);
                        self.node_audit_buf.push(nx);
                        self.node_audit_buf.push(ny);
                    }
                }
                i = j;
            }

            // Write group_count into first slot
            self.node_audit_buf[0] = group_count as f32;
        }

        // Compute degrees and leaf stripping
        self.node_degree.clear();
        self.allow_node.clear();
        self.effective_degree.clear();

        let node_len = self.fill_graph.nodes.len();
        self.node_degree.resize(node_len, 0);
        self.effective_degree.resize(node_len, 0);
        self.allow_node.resize(node_len, true);

        for (i, outs) in self.fill_graph.node_sectors.iter().enumerate() {
            let deg = outs.len() as u32;
            self.node_degree[i] = deg;
            self.effective_degree[i] = deg;
        }

        // Leaf stripping
        let mut queue: Vec<u32> = Vec::new();
        for (i, deg) in self.effective_degree.iter().enumerate() {
            if *deg <= 1 {
                queue.push(i as u32);
            }
        }

        while let Some(nid) = queue.pop() {
            let idx = nid as usize;
            if idx >= self.allow_node.len() {
                continue;
            }
            if !self.allow_node[idx] {
                continue;
            }
            self.allow_node[idx] = false;

            // Reduce neighbor degrees
            if idx < self.fill_graph.outgoing.len() {
                let neighbors = self.fill_graph.outgoing[idx].clone();
                for he_idx in neighbors.iter() {
                    let he = self.fill_graph.half_edges[*he_idx as usize];
                    let nb = he.to as usize;
                    if self.effective_degree[nb] > 0 {
                        self.effective_degree[nb] -= 1;
                        if self.effective_degree[nb] <= 1 {
                            queue.push(nb as u32);
                        }
                    }
                }
            }
        }

        self.rebuild_graph_debug_buf();
        
        // Verify fill graph integrity in debug builds
        check_fill_graph_integrity(&self.fill_graph);
    }

    fn rebuild_graph_debug_buf(&mut self) {
        self.graph_debug_buf.clear();

        // Segments
        let seg_count = self.fill_graph.segments.len() as f32;
        self.graph_debug_buf.push(seg_count);
        for (idx, seg) in self.fill_graph.segments.iter().enumerate() {
            let a = self.fill_graph.nodes[seg.a as usize];
            let b = self.fill_graph.nodes[seg.b as usize];
            self.graph_debug_buf.push(a.x);
            self.graph_debug_buf.push(a.y);
            self.graph_debug_buf.push(b.x);
            self.graph_debug_buf.push(b.y);
            self.graph_debug_buf.push(idx as f32);
        }

        // Nodes
        let node_count = self.fill_graph.nodes.len() as f32;
        self.graph_debug_buf.push(node_count);
        for (idx, n) in self.fill_graph.nodes.iter().enumerate() {
            self.graph_debug_buf.push(n.x);
            self.graph_debug_buf.push(n.y);
            self.graph_debug_buf.push(idx as f32);
        }
    }

    fn trace_face_side(
        &mut self,
        start_from: u32,
        start_to: u32,
        side_rule: SideRule,
        origin: (f32, f32),
        gap_radius: f32,
    ) -> TraceResult {
        #[derive(Clone)]
        struct TraceCandidate {
            next: u32,
            turn: f32,
            abs_turn: f32,
            out_x: f32,
            out_y: f32,
            is_virtual: bool,
            visited: bool,
            he_idx: Option<u32>,
            seg_id: Option<u32>,
            len: f32,
            mid_dist: f32,
            dist2: f32,
            accepted_side: bool,
        }

        #[derive(Clone, Copy, PartialEq)]
        struct EdgeKey {
            from: u32,
            to: u32,
            seg: Option<u32>,
            is_virtual: bool,
        }

        const ANG_EPS: f32 = 1e-3;

        fn pick_candidate_idx(cands: &[TraceCandidate], indices: &[usize]) -> Option<usize> {
            if indices.is_empty() {
                return None;
            }
            let mut best = indices[0];
            for &idx in indices.iter().skip(1) {
                let cand = &cands[idx];
                let best_cand = &cands[best];
                let mut better = false;
                if cand.abs_turn + 1e-6 < best_cand.abs_turn {
                    better = true;
                } else if absf(cand.abs_turn - best_cand.abs_turn) <= 1e-6 {
                    if !cand.is_virtual && best_cand.is_virtual {
                        better = true;
                    } else if cand.is_virtual == best_cand.is_virtual {
                        if cand.len + 1e-4 < best_cand.len {
                            better = true;
                        } else if absf(cand.len - best_cand.len) <= 1e-4 {
                            if cand.mid_dist + 1e-4 < best_cand.mid_dist {
                                better = true;
                            } else if absf(cand.mid_dist - best_cand.mid_dist) <= 1e-4 {
                                if cand.next < best_cand.next {
                                    better = true;
                                } else if cand.next == best_cand.next {
                                    let cand_he = cand.he_idx.unwrap_or(u32::MAX);
                                    let best_he = best_cand.he_idx.unwrap_or(u32::MAX);
                                    if cand_he < best_he {
                                        better = true;
                                    }
                                }
                            }
                        }
                    }
                }
                if better {
                    best = idx;
                }
            }
            Some(best)
        }

        fn turn_angle_signed(
            vinx: f32,
            viny: f32,
            vinlen: f32,
            voutx: f32,
            vouty: f32,
            voutlen: f32,
        ) -> f32 {
            if vinlen < 1e-6 || voutlen < 1e-6 {
                return 0.0;
            }
            let dotp = vinx * voutx + viny * vouty;
            let crossp = vinx * vouty - viny * voutx;
            atan2_approx(crossp, dotp)
        }

        fn edge_visited(visited: &[EdgeKey], key: EdgeKey) -> bool {
            for e in visited.iter() {
                if *e == key {
                    return true;
                }
            }
            false
        }

        fn find_segment_id(graph: &FillGraph, from: u32, to: u32) -> Option<u32> {
            if (from as usize) >= graph.node_sectors.len() {
                return None;
            }
            for sid in graph.node_sectors[from as usize].iter() {
                let seg = graph.segments[*sid as usize];
                if (seg.a == from && seg.b == to) || (seg.b == from && seg.a == to) {
                    return Some(*sid);
                }
            }
            None
        }

        let mut boundary_points: Vec<(f32, f32)> = Vec::new();
        let mut visited_edges: Vec<EdgeKey> = Vec::new();
        let mut step_debug: Vec<StepDebug> = Vec::new();

        if (start_from as usize) >= self.fill_graph.nodes.len()
            || (start_to as usize) >= self.fill_graph.nodes.len()
        {
            return TraceResult::fail(FailReason::DeadEnd, 0, step_debug);
        }

        let mut cur_node = start_to;
        let mut prev_node = start_from;
        let cur_pt = self.fill_graph.nodes[cur_node as usize];
        boundary_points.push((cur_pt.x, cur_pt.y));

        let start_seg = find_segment_id(&self.fill_graph, start_from, start_to);
        let start_edge = EdgeKey {
            from: start_from,
            to: start_to,
            seg: start_seg,
            is_virtual: false,
        };
        visited_edges.push(start_edge);

        let max_steps = core::cmp::max(
            8,
            (self.fill_graph.segments.len() as u32) * 3 + 20,
        );
        let mut step_idx = 0u32;
        let gap_r2 = gap_radius * gap_radius;

        loop {
            if step_idx >= max_steps {
                return TraceResult::fail(FailReason::StepLimit, step_idx, step_debug);
            }

            let cur_pt = self.fill_graph.nodes[cur_node as usize];
            let prev_pt = self.fill_graph.nodes[prev_node as usize];
            let (vinx, viny, vinlen) = normalize_with_len(cur_pt.x - prev_pt.x, cur_pt.y - prev_pt.y);

            let mut usable: Vec<TraceCandidate> = Vec::new();

            if (cur_node as usize) < self.fill_graph.outgoing.len() {
                for he_idx in self.fill_graph.outgoing[cur_node as usize].iter() {
                    let he = self.fill_graph.half_edges[*he_idx as usize];
                    let next_node = he.to;
                    if next_node == prev_node {
                        continue;
                    }
                    let next_pt = self.fill_graph.nodes[next_node as usize];
                    let (voutx, vouty, voutlen) =
                        normalize_with_len(next_pt.x - cur_pt.x, next_pt.y - cur_pt.y);
                    if voutlen < 1e-6 {
                        continue;
                    }
                    let turn = turn_angle_signed(vinx, viny, vinlen, voutx, vouty, voutlen);
                    let abs_turn = absf(turn);
                    let eff_deg = if (next_node as usize) < self.effective_degree.len() {
                        self.effective_degree[next_node as usize]
                    } else {
                        0
                    };
                    let allow = if (next_node as usize) < self.allow_node.len() {
                        self.allow_node[next_node as usize]
                    } else {
                        true
                    };
                    if eff_deg <= 1 || !allow {
                        continue;
                    }
                    let len = sqrt_approx(distance_sq(cur_pt.x, cur_pt.y, next_pt.x, next_pt.y));
                    if len < 1e-6 {
                        continue;
                    }
                    let midx = (cur_pt.x + next_pt.x) * 0.5;
                    let midy = (cur_pt.y + next_pt.y) * 0.5;
                    let mid_dist = sqrt_approx(distance_sq(origin.0, origin.1, midx, midy));
                    let accepted_side = match side_rule {
                        SideRule::KeepLeft => turn > ANG_EPS,
                        SideRule::KeepRight => turn < -ANG_EPS,
                    };
                    let key = EdgeKey {
                        from: cur_node,
                        to: next_node,
                        seg: Some(he.seg),
                        is_virtual: false,
                    };
                    let already = edge_visited(&visited_edges, key);
                    let is_closure_edge = key == start_edge;
                    if already && !is_closure_edge {
                        continue;
                    }
                    usable.push(TraceCandidate {
                        next: next_node,
                        turn,
                        abs_turn,
                        out_x: voutx,
                        out_y: vouty,
                        is_virtual: false,
                        visited: already,
                        he_idx: Some(*he_idx),
                        seg_id: Some(he.seg),
                        len,
                        mid_dist,
                        dist2: len * len,
                        accepted_side,
                    });
                }
            }

            if usable.is_empty() {
                let mut gap_candidates: Vec<TraceCandidate> = Vec::new();
                let mut best_dist = f32::INFINITY;
                for (nid, node) in self.fill_graph.nodes.iter().enumerate() {
                    let nid_u = nid as u32;
                    if nid_u == cur_node || nid_u == prev_node {
                        continue;
                    }
                    if (nid as usize) >= self.effective_degree.len() {
                        continue;
                    }
                    let eff_deg = self.effective_degree[nid];
                    let allow = if nid < self.allow_node.len() {
                        self.allow_node[nid]
                    } else {
                        true
                    };
                    if eff_deg < 2 || !allow {
                        continue;
                    }
                    let dx = node.x - cur_pt.x;
                    let dy = node.y - cur_pt.y;
                    let dist2 = dx * dx + dy * dy;
                    if dist2 > gap_r2 {
                        continue;
                    }
                    if dist2 < best_dist {
                        best_dist = dist2;
                    }
                    let (voutx, vouty, voutlen) = normalize_with_len(dx, dy);
                    if voutlen < 1e-6 {
                        continue;
                    }
                    let turn = turn_angle_signed(vinx, viny, vinlen, voutx, vouty, voutlen);
                    let abs_turn = absf(turn);
                    let accepted_side = match side_rule {
                        SideRule::KeepLeft => turn > ANG_EPS,
                        SideRule::KeepRight => turn < -ANG_EPS,
                    };
                    let key = EdgeKey {
                        from: cur_node,
                        to: nid_u,
                        seg: None,
                        is_virtual: true,
                    };
                    if edge_visited(&visited_edges, key) {
                        continue;
                    }
                    let len = sqrt_approx(dist2);
                    let midx = (cur_pt.x + node.x) * 0.5;
                    let midy = (cur_pt.y + node.y) * 0.5;
                    let mid_dist = sqrt_approx(distance_sq(origin.0, origin.1, midx, midy));
                    gap_candidates.push(TraceCandidate {
                        next: nid_u,
                        turn,
                        abs_turn,
                        out_x: voutx,
                        out_y: vouty,
                        is_virtual: true,
                        visited: false,
                        he_idx: None,
                        seg_id: None,
                        len,
                        mid_dist,
                        dist2,
                        accepted_side,
                    });
                }

                if !gap_candidates.is_empty() && best_dist.is_finite() {
                    for gc in gap_candidates.into_iter() {
                        if gc.dist2 <= best_dist + 1e-3 {
                            usable.push(gc);
                        }
                    }
                }
            }

            if usable.is_empty() {
                return TraceResult::fail(FailReason::DeadEnd, step_idx, step_debug);
            }

            let mut side_indices: Vec<usize> = Vec::new();
            let mut all_indices: Vec<usize> = Vec::new();
            for (idx, cand) in usable.iter().enumerate() {
                all_indices.push(idx);
                if cand.accepted_side {
                    side_indices.push(idx);
                }
            }
            let pool_indices = if !side_indices.is_empty() {
                side_indices.as_slice()
            } else {
                all_indices.as_slice()
            };

            let chosen_idx = match pick_candidate_idx(&usable, pool_indices) {
                Some(i) => i,
                None => return TraceResult::fail(FailReason::DeadEnd, step_idx, step_debug),
            };

            let chosen = usable[chosen_idx].clone();
            let selected_key = EdgeKey {
                from: cur_node,
                to: chosen.next,
                seg: chosen.seg_id,
                is_virtual: chosen.is_virtual,
            };

            let mut step_cands: Vec<StepCandidateDebug> = Vec::new();
            for (idx, cand) in usable.iter().enumerate() {
                step_cands.push(StepCandidateDebug {
                    next: cand.next,
                    turn: cand.turn,
                    abs_turn: cand.abs_turn,
                    out_x: cand.out_x,
                    out_y: cand.out_y,
                    is_virtual: cand.is_virtual,
                    accepted_side: cand.accepted_side,
                    was_visited: cand.visited,
                    chosen: idx == chosen_idx,
                    len: cand.len,
                    mid_dist: cand.mid_dist,
                });
            }
            step_debug.push(StepDebug {
                step_idx,
                cur: cur_node,
                prev: prev_node,
                vin: (vinx, viny),
                candidates: step_cands,
            });

            let next_pt = self.fill_graph.nodes[chosen.next as usize];
            boundary_points.push((next_pt.x, next_pt.y));

            if selected_key == start_edge && step_idx >= 1 {
                // Safety check: ensure we visited at least 3 unique nodes
                // This prevents degenerate loops from dangling chains
                let mut unique_nodes: Vec<u32> = Vec::new();
                unique_nodes.push(start_from);
                for edge in visited_edges.iter() {
                    let mut found_from = false;
                    let mut found_to = false;
                    for &n in unique_nodes.iter() {
                        if n == edge.from {
                            found_from = true;
                        }
                        if n == edge.to {
                            found_to = true;
                        }
                    }
                    if !found_from {
                        unique_nodes.push(edge.from);
                    }
                    if !found_to {
                        unique_nodes.push(edge.to);
                    }
                }
                
                if unique_nodes.len() < 3 {
                    return TraceResult::fail(FailReason::DeadEnd, step_idx, step_debug);
                }
                
                return TraceResult::success(boundary_points, step_idx + 1, step_debug);
            }

            if edge_visited(&visited_edges, selected_key) {
                return TraceResult::fail(FailReason::PrematureCycle, step_idx, step_debug);
            }

            visited_edges.push(selected_key);
            prev_node = cur_node;
            cur_node = chosen.next;
            step_idx += 1;
        }
    }

    fn fill_debug_at(&mut self, ox: f32, oy: f32) {
        // Validate input coordinates
        check_line_coordinates(ox, oy, ox, oy);
        
        self.fill_trace_buf.clear();
        self.fill_walk_debug_buf.clear();
        self.fill_candidates_buf.clear();
        self.adjacency_debug_buf.clear();
        self.build_fill_graph();

        // trace count placeholder for step debug log
        self.fill_candidates_buf.push(0.0);

        // Step 0: origin
        self.fill_trace_buf.push(ox);
        self.fill_trace_buf.push(oy);
        self.fill_trace_buf.push(0.0); // type = origin

        if self.fill_graph.segments.is_empty() {
            let mut result = Vec::new();
            result.push((self.fill_trace_buf.len() / 3) as f32);
            result.extend(self.fill_trace_buf.drain(..));
            self.fill_trace_buf = result;
            self.fill_candidates_buf[0] = 0.0;
            return;
        }

        // Pick starting segment nearest to origin
        // CRITICAL: only consider segments where BOTH endpoints have degree >= 2
        // This prevents starting fill from dangling edges (open chains)
        
        let mut candidates: Vec<(usize, f32)> = Vec::new();
        for (i, seg) in self.fill_graph.segments.iter().enumerate() {
            let degree_a = if (seg.a as usize) < self.fill_graph.node_sectors.len() {
                self.fill_graph.node_sectors[seg.a as usize].len()
            } else {
                0
            };
            let degree_b = if (seg.b as usize) < self.fill_graph.node_sectors.len() {
                self.fill_graph.node_sectors[seg.b as usize].len()
            } else {
                0
            };

            // Only accept segments where both endpoints have degree >= 2
            if degree_a >= 2 && degree_b >= 2 {
                let a = self.fill_graph.nodes[seg.a as usize];
                let b = self.fill_graph.nodes[seg.b as usize];
                
                // Compute distance to segment interior (not endpoints)
                // Using parametric distance to line segment
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let seg_len_sq = dx * dx + dy * dy;
                
                let d2 = if seg_len_sq > 1e-9 {
                    // Project click point onto segment
                    let t = ((ox - a.x) * dx + (oy - a.y) * dy) / seg_len_sq;
                    let t_clamped = if t < 0.0 { 0.0 } else if t > 1.0 { 1.0 } else { t };
                    let proj_x = a.x + t_clamped * dx;
                    let proj_y = a.y + t_clamped * dy;
                    distance_sq(ox, oy, proj_x, proj_y)
                } else {
                    // Degenerate segment (point)
                    distance_sq(ox, oy, a.x, a.y)
                };
                
                candidates.push((i, d2));
            }
        }

        // Sort by distance and pick closest valid segment
        if candidates.is_empty() {
            // No valid boundary edge found - abort fill
            // Fill aborted: no valid boundary edge near cursor
            let mut result = Vec::new();
            result.push((self.fill_trace_buf.len() / 3) as f32);
            result.extend(self.fill_trace_buf.drain(..));
            self.fill_trace_buf = result;
            self.fill_candidates_buf[0] = 0.0;
            return;
        }

        candidates.sort_by(|a, b| {
            if a.1 < b.1 { core::cmp::Ordering::Less }
            else if a.1 > b.1 { core::cmp::Ordering::Greater }
            else { core::cmp::Ordering::Equal }
        });

        let best_seg_idx = candidates[0].0;
        let start_seg = self.fill_graph.segments[best_seg_idx];
        let node_a = start_seg.a;
        let node_b = start_seg.b;

        let pa = self.fill_graph.nodes[node_a as usize];
        let pb = self.fill_graph.nodes[node_b as usize];
        let cross_ab = cross(pb.x - pa.x, pb.y - pa.y, ox - pa.x, oy - pa.y);
        let cross_ba = cross(pa.x - pb.x, pa.y - pb.y, ox - pb.x, oy - pb.y);
        let side_ab = if cross_ab > 0.0 { SideRule::KeepLeft } else { SideRule::KeepRight };
        let side_ba = if cross_ba > 0.0 { SideRule::KeepLeft } else { SideRule::KeepRight };
        let origin = (ox, oy);

        // DUAL-TRACE: Try both directions with side-locking
        let result_ab = self.trace_face_side(node_a, node_b, side_ab, origin, GAP_RADIUS);
        let result_ba = self.trace_face_side(node_b, node_a, side_ba, origin, GAP_RADIUS);

        let origin = (ox, oy);

        // Debug log: start direction and side
        let mut trace_counter = 0u32;
        {
            let side_code = if let SideRule::KeepLeft = side_ab { 0.0 } else { 1.0 };
            trace_counter += 1;
            self.fill_candidates_buf.push(trace_counter as f32);
            self.fill_candidates_buf.push(node_a as f32);
            self.fill_candidates_buf.push(node_b as f32);
            self.fill_candidates_buf.push(side_code);
            self.fill_candidates_buf.push(cross_ab);
            self.fill_candidates_buf.push(if result_ab.closed { 1.0 } else { 0.0 });
            self.fill_candidates_buf.push(result_ab.steps as f32);
            self.fill_candidates_buf.push(result_ab.step_debug.len() as f32);
            for step in result_ab.step_debug.iter() {
                self.fill_candidates_buf.push(step.step_idx as f32);
                self.fill_candidates_buf.push(step.cur as f32);
                self.fill_candidates_buf.push(step.prev as f32);
                self.fill_candidates_buf.push(step.vin.0);
                self.fill_candidates_buf.push(step.vin.1);
                self.fill_candidates_buf.push(step.candidates.len() as f32);
                for cand in step.candidates.iter() {
                    self.fill_candidates_buf.push(cand.next as f32);
                    self.fill_candidates_buf.push(cand.turn);
                    self.fill_candidates_buf.push(cand.abs_turn);
                    self.fill_candidates_buf.push(if cand.is_virtual { 1.0 } else { 0.0 });
                    self.fill_candidates_buf.push(cand.out_x);
                    self.fill_candidates_buf.push(cand.out_y);
                    self.fill_candidates_buf.push(if cand.accepted_side { 1.0 } else { 0.0 });
                    self.fill_candidates_buf.push(if cand.was_visited { 1.0 } else { 0.0 });
                    self.fill_candidates_buf.push(cand.len);
                    self.fill_candidates_buf.push(cand.mid_dist);
                    self.fill_candidates_buf.push(if cand.chosen { 1.0 } else { 0.0 });
                }
            }
        }

        {
            let side_code = if let SideRule::KeepLeft = side_ba { 0.0 } else { 1.0 };
            trace_counter += 1;
            self.fill_candidates_buf.push(trace_counter as f32);
            self.fill_candidates_buf.push(node_b as f32);
            self.fill_candidates_buf.push(node_a as f32);
            self.fill_candidates_buf.push(side_code);
            self.fill_candidates_buf.push(cross_ba);
            self.fill_candidates_buf.push(if result_ba.closed { 1.0 } else { 0.0 });
            self.fill_candidates_buf.push(result_ba.steps as f32);
            self.fill_candidates_buf.push(result_ba.step_debug.len() as f32);
            for step in result_ba.step_debug.iter() {
                self.fill_candidates_buf.push(step.step_idx as f32);
                self.fill_candidates_buf.push(step.cur as f32);
                self.fill_candidates_buf.push(step.prev as f32);
                self.fill_candidates_buf.push(step.vin.0);
                self.fill_candidates_buf.push(step.vin.1);
                self.fill_candidates_buf.push(step.candidates.len() as f32);
                for cand in step.candidates.iter() {
                    self.fill_candidates_buf.push(cand.next as f32);
                    self.fill_candidates_buf.push(cand.turn);
                    self.fill_candidates_buf.push(cand.abs_turn);
                    self.fill_candidates_buf.push(if cand.is_virtual { 1.0 } else { 0.0 });
                    self.fill_candidates_buf.push(cand.out_x);
                    self.fill_candidates_buf.push(cand.out_y);
                    self.fill_candidates_buf.push(if cand.accepted_side { 1.0 } else { 0.0 });
                    self.fill_candidates_buf.push(if cand.was_visited { 1.0 } else { 0.0 });
                    self.fill_candidates_buf.push(cand.len);
                    self.fill_candidates_buf.push(cand.mid_dist);
                    self.fill_candidates_buf.push(if cand.chosen { 1.0 } else { 0.0 });
                }
            }
        }

        if !self.fill_candidates_buf.is_empty() {
            self.fill_candidates_buf[0] = trace_counter as f32;
        }

        // COMPUTE DIAGNOSTICS for both candidates
        let diag_ab = if result_ab.closed && result_ab.points.len() >= 3 {
            let area = absf(signed_area_2x(&result_ab.points)) * 0.5;
            let inside = point_in_poly_evenodd(origin, &result_ab.points);
            let dist_sq = min_dist_sq_to_polygon(origin, &result_ab.points);
            let (minx, miny, maxx, maxy) = poly_bounds(&result_ab.points);
            let is_simple = is_simple_polygon(&result_ab.points);
            Some((1.0, result_ab.points.len() as f32, absf(signed_area_2x(&result_ab.points)), area, 
                  if inside { 1.0 } else { 0.0 }, dist_sq, sqrt_approx(dist_sq), 
                  if is_simple { 1.0 } else { 0.0 }, minx, miny, maxx, maxy, inside, area))
        } else {
            None
        };

        let diag_ba = if result_ba.closed && result_ba.points.len() >= 3 {
            let area = absf(signed_area_2x(&result_ba.points)) * 0.5;
            let inside = point_in_poly_evenodd(origin, &result_ba.points);
            let dist_sq = min_dist_sq_to_polygon(origin, &result_ba.points);
            let (minx, miny, maxx, maxy) = poly_bounds(&result_ba.points);
            let is_simple = is_simple_polygon(&result_ba.points);
            Some((2.0, result_ba.points.len() as f32, absf(signed_area_2x(&result_ba.points)), area,
                  if inside { 1.0 } else { 0.0 }, dist_sq, sqrt_approx(dist_sq),
                  if is_simple { 1.0 } else { 0.0 }, minx, miny, maxx, maxy, inside, area))
        } else {
            None
        };

        // Store diagnostics for visualization
        if let Some(d) = &diag_ab {
            self.fill_walk_debug_buf.clear();
            self.fill_walk_debug_buf.push(d.0); // direction
            self.fill_walk_debug_buf.push(d.1); // points.len
            self.fill_walk_debug_buf.push(d.2); // signed_area*2
            self.fill_walk_debug_buf.push(d.3); // area
            self.fill_walk_debug_buf.push(d.4); // inside
            self.fill_walk_debug_buf.push(d.5); // dist_sq
            self.fill_walk_debug_buf.push(d.6); // dist
            self.fill_walk_debug_buf.push(d.7); // is_simple
            self.fill_walk_debug_buf.push(d.8); // minx
            self.fill_walk_debug_buf.push(d.9); // miny
            self.fill_walk_debug_buf.push(d.10); // maxx
            self.fill_walk_debug_buf.push(d.11); // maxy
        }
        if let Some(d) = &diag_ba {
            self.fill_walk_debug_buf.push(d.0);
            self.fill_walk_debug_buf.push(d.1);
            self.fill_walk_debug_buf.push(d.2);
            self.fill_walk_debug_buf.push(d.3);
            self.fill_walk_debug_buf.push(d.4);
            self.fill_walk_debug_buf.push(d.5);
            self.fill_walk_debug_buf.push(d.6);
            self.fill_walk_debug_buf.push(d.7);
            self.fill_walk_debug_buf.push(d.8);
            self.fill_walk_debug_buf.push(d.9);
            self.fill_walk_debug_buf.push(d.10);
            self.fill_walk_debug_buf.push(d.11);
        }

        // Validate results
        let mut valid_results: Vec<(bool, bool, f32, &TraceResult)> = Vec::new();

        if result_ab.closed && result_ab.points.len() >= 3 {
            let area = absf(signed_area_2x(&result_ab.points)) * 0.5;
            let inside = point_in_poly_evenodd(origin, &result_ab.points);
            if area > MIN_AREA && inside {
                valid_results.push((true, inside, area, &result_ab));
            }
        }

        if result_ba.closed && result_ba.points.len() >= 3 {
            let area = absf(signed_area_2x(&result_ba.points)) * 0.5;
            let inside = point_in_poly_evenodd(origin, &result_ba.points);
            if area > MIN_AREA && inside {
                valid_results.push((true, inside, area, &result_ba));
            }
        }

        // Selection logic
        let selected: Option<&TraceResult> = if valid_results.is_empty() {
            None
        } else if valid_results.len() == 1 {
            Some(valid_results[0].3)
        } else {
            let inside_results: Vec<_> = valid_results.iter().filter(|(_, inside, _, _)| *inside).collect();
            if !inside_results.is_empty() {
                inside_results.iter().min_by(|a, b| {
                    let a_area = a.2;
                    let b_area = b.2;
                    if a_area < b_area { core::cmp::Ordering::Less } else { core::cmp::Ordering::Greater }
                }).map(|x| x.3)
            } else {
                Some(valid_results[0].3)
            }
        };

        // Log and create polygon
        if let Some(sel) = selected {
            let pt_a = self.fill_graph.nodes[node_a as usize];
            self.fill_trace_buf.push(pt_a.x);
            self.fill_trace_buf.push(pt_a.y);
            self.fill_trace_buf.push(2.0);
            
            for pt in sel.points.iter().skip(1) {
                self.fill_trace_buf.push(pt.0);
                self.fill_trace_buf.push(pt.1);
                self.fill_trace_buf.push(3.0);
            }

            self.fill_trace_buf.push(pt_a.x);
            self.fill_trace_buf.push(pt_a.y);
            self.fill_trace_buf.push(9.0);

            let mut result = Vec::new();
            result.push((self.fill_trace_buf.len() / 3) as f32);
            result.extend(self.fill_trace_buf.drain(..));
            self.fill_trace_buf = result;
            
            self.create_polygon_from_selected(&sel.points);
        } else {
            let mut result = Vec::new();
            result.push((self.fill_trace_buf.len() / 3) as f32);
            result.extend(self.fill_trace_buf.drain(..));
            self.fill_trace_buf = result;
        }
    }


    #[allow(dead_code)]
    fn create_polygon_from_trace(&mut self) {
        // Extract polygon points from fill_trace_buf
        // Only include boundary points: step_type 2.0 (start node) and 3.0 (chain points)
        // Exclude: 0.0 (origin/click point), 7.0 (step limit), 8.0 (dead end), 9.0 (closure marker)
        let mut polygon = Polygon::new();
        polygon.color = self.fill_color;

        let mut i = 0;
        let mut first_point: Option<(f32, f32)> = None;
        
        while i + 2 < self.fill_trace_buf.len() {
            let x = self.fill_trace_buf[i];
            let y = self.fill_trace_buf[i + 1];
            let step_type = self.fill_trace_buf[i + 2];

            // Only include boundary traversal points (start node and chain points)
            if step_type == 2.0 || step_type == 3.0 {
                if first_point.is_none() {
                    first_point = Some((x, y));
                }
                polygon.points.push((x, y));
            }

            i += 3;
        }

        // Close polygon by appending first point at end
        if let Some(first) = first_point {
            if polygon.points.len() >= 3 && polygon.points.last() != Some(&first) {
                polygon.points.push(first);
            }
        }

        if polygon.points.len() >= 4 {
            self.fills.push(polygon);
            self.history.push(Command::AddFill);
            self.refresh_export_fills();
        }
    }

    fn create_polygon_from_selected(&mut self, points: &[(f32, f32)]) {
        // Create polygon directly from selected TraceResult points
        let mut polygon = Polygon::new();
        polygon.color = self.fill_color;
        
        if points.len() >= 3 {
            polygon.points = points.to_vec();
            
            // Ensure closure
            if polygon.points.len() >= 3 && polygon.points.first() != polygon.points.last() {
                polygon.points.push(polygon.points[0]);
            }
            
            self.fills.push(polygon);
            self.history.push(Command::AddFill);
            self.refresh_export_fills();
        }
    }

    fn refresh_export_fills(&mut self) {
        self.refresh_fills_export_buf();
    }

    fn refresh_export(&mut self) {
        self.export_buf.clear();
        self.export_buf.reserve(self.lines.len() * 4);
        for line in self.lines.iter() {
            self.export_buf.push(line.x1);
            self.export_buf.push(line.y1);
            self.export_buf.push(line.x2);
            self.export_buf.push(line.y2);
        }
    }

    #[allow(dead_code)]
    fn export_fills(&mut self) -> Vec<f32> {
        let mut result: Vec<f32> = Vec::new();
        for polygon in self.fills.iter() {
            result.push(polygon.points.len() as f32);
            result.push((polygon.color >> 24) as u8 as f32 / 255.0);
            result.push(((polygon.color >> 16) & 0xFF) as u8 as f32 / 255.0);
            result.push(((polygon.color >> 8) & 0xFF) as u8 as f32 / 255.0);
            result.push((polygon.color & 0xFF) as u8 as f32 / 255.0);
            for (x, y) in polygon.points.iter() {
                result.push(*x);
                result.push(*y);
            }
        }
        result
    }

    fn refresh_fills_export_buf(&mut self) {
        self.fills_export_buf.clear();
        for polygon in self.fills.iter() {
            self.fills_export_buf.push(polygon.points.len() as f32);
            self.fills_export_buf.push((polygon.color >> 24) as u8 as f32 / 255.0);
            self.fills_export_buf.push(((polygon.color >> 16) & 0xFF) as u8 as f32 / 255.0);
            self.fills_export_buf.push(((polygon.color >> 8) & 0xFF) as u8 as f32 / 255.0);
            self.fills_export_buf.push((polygon.color & 0xFF) as u8 as f32 / 255.0);
            for (x, y) in polygon.points.iter() {
                self.fills_export_buf.push(*x);
                self.fills_export_buf.push(*y);
            }
        }
    }

    // Recompute and cache intersection points whenever geometry changes
    fn recompute_intersections(&mut self) {
        // Intersection export is rebuilt inside build_fill_graph now.
        self.intersections.clear();
        self.intersections_export.clear();
    }

    fn compute_nearest_debug(&mut self, px: f32, py: f32) {
        self.debug_buf.clear();

        if self.lines.is_empty() {
            self.debug_buf.push(0.0);
            return;
        }

        let mut best_line_idx = 0;
        let mut best_t = 0.0;
        let mut best_qx = 0.0;
        let mut best_qy = 0.0;
        let mut best_dist2 = f32::INFINITY;

        for (i, line) in self.lines.iter().enumerate() {
            let (t, qx, qy, dist2) = point_segment_nearest(px, py, line.x1, line.y1, line.x2, line.y2);
            if dist2 < best_dist2 {
                best_dist2 = dist2;
                best_line_idx = i;
                best_t = t;
                best_qx = qx;
                best_qy = qy;
            }
        }

        let line = &self.lines[best_line_idx];
        self.debug_buf.push(1.0); // hit flag
        self.debug_buf.push(line.x1);
        self.debug_buf.push(line.y1);
        self.debug_buf.push(line.x2);
        self.debug_buf.push(line.y2);
        self.debug_buf.push(best_qx);
        self.debug_buf.push(best_qy);
        self.debug_buf.push(best_dist2);
        self.debug_buf.push(best_t);
    }

    fn add_line(&mut self, line: Line) {
        // Validate coordinates
        check_line_coordinates(line.x1, line.y1, line.x2, line.y2);
        
        // Skip zero-length lines
        let dx = line.x2 - line.x1;
        let dy = line.y2 - line.y1;
        if dx * dx + dy * dy < 1.0 {
            return;
        }
        
        self.lines.push(line);
        self.history.push(Command::Add);
        self.refresh_export();
        self.recompute_intersections();
        self.build_fill_graph();
    }

    fn add_frame(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32) {
        // Add 4 lines forming a rectangle frame
        // Corners in clockwise order: (x1,y1), (x2,y2), (x3,y3), (x4,y4)
        self.lines.push(Line { x1, y1, x2, y2 }); // top
        self.lines.push(Line { x1: x2, y1: y2, x2: x3, y2: y3 }); // right
        self.lines.push(Line { x1: x3, y1: y3, x2: x4, y2: y4 }); // bottom
        self.lines.push(Line { x1: x4, y1: y4, x2: x1, y2: y1 }); // left
        
        // Push single grouped undo command
        self.history.push(Command::AddFrame);
        self.refresh_export();
        self.recompute_intersections();
        self.build_fill_graph();
    }

    fn clear(&mut self) {
        let previous_lines = self.lines.clone();
        let previous_fills = self.fills.clone();
        self.lines.clear();
        self.fills.clear();
        self.history.push(Command::Clear(previous_lines, previous_fills));
        self.fill_trace_buf.clear();
        self.refresh_export();
        self.recompute_intersections();
        self.build_fill_graph();
        
        // Verify cleared state in debug builds
        #[cfg(debug_assertions)]
        {
            debug_assert!(self.lines.is_empty(), "Lines not cleared");
            debug_assert!(self.fills.is_empty(), "Fills not cleared");
        }
    }

    // Overhang trimming = 2-core leaf stripping on cut-segment graph
    fn cleanup_overhangs(&mut self) {
        // Save current state for undo
        let previous_lines = self.lines.clone();
        
        if self.lines.is_empty() {
            return;
        }
        
        // Build the cut segment graph (same as fill uses)
        self.recompute_intersections();
        self.build_fill_graph();
        
        let graph = &self.fill_graph;
        let total_segments = graph.segments.len();
        
        if total_segments == 0 {
            return;
        }
        
        // Compute initial node degrees from segments
        let mut degree: Vec<u32> = Vec::new();
        degree.resize(graph.nodes.len(), 0);
        
        for seg in &graph.segments {
            let a_idx = seg.a as usize;
            let b_idx = seg.b as usize;
            if a_idx < degree.len() {
                degree[a_idx] += 1;
            }
            if b_idx < degree.len() {
                degree[b_idx] += 1;
            }
        }
        
        // Initialize queue with all nodes where degree <= 1 (leaves and isolated)
        let mut queue: Vec<usize> = Vec::new();
        for (node_idx, &deg) in degree.iter().enumerate() {
            if deg <= 1 {
                queue.push(node_idx);
            }
        }
        
        // Mark segments for deletion via 2-core leaf stripping
        let mut delete_segment: Vec<bool> = Vec::new();
        delete_segment.resize(total_segments, false);
        let mut deleted_count: u32 = 0;
        
        while let Some(node_idx) = queue.pop() {
            // Re-check degree (may have changed)
            if degree[node_idx] > 1 {
                continue;
            }
            
            if degree[node_idx] == 0 {
                // Isolated node, nothing to do
                continue;
            }
            
            // degree[node_idx] == 1: find the single alive segment
            let mut alive_segment_idx: Option<usize> = None;
            for (seg_idx, &is_deleted) in delete_segment.iter().enumerate() {
                if is_deleted {
                    continue;
                }
                let seg = &graph.segments[seg_idx];
                let a_idx = seg.a as usize;
                let b_idx = seg.b as usize;
                if a_idx == node_idx || b_idx == node_idx {
                    alive_segment_idx = Some(seg_idx);
                    break;
                }
            }
            
            if let Some(seg_idx) = alive_segment_idx {
                let seg = &graph.segments[seg_idx];
                let a_idx = seg.a as usize;
                let b_idx = seg.b as usize;
                let other_idx = if a_idx == node_idx { b_idx } else { a_idx };
                
                // Mark segment for deletion
                delete_segment[seg_idx] = true;
                deleted_count += 1;
                
                // Update degrees
                degree[node_idx] = 0;
                if other_idx < degree.len() && degree[other_idx] > 0 {
                    degree[other_idx] -= 1;
                    // If other node becomes a leaf, add to queue
                    if degree[other_idx] <= 1 {
                        queue.push(other_idx);
                    }
                }
            }
        }
        
        let kept_count = total_segments - (deleted_count as usize);
        
        // Store debug info (can be logged in debug mode)
        // Using existing debug buffer or could create new one
        self.debug_buf.clear();
        self.debug_buf.push(total_segments as f32);
        self.debug_buf.push(deleted_count as f32);
        self.debug_buf.push(kept_count as f32);
        self.debug_buf.push(graph.nodes.len() as f32);
        
        if deleted_count == 0 {
            // No changes - don't push undo
            return;
        }
        
        // Collect all KEPT segments as new lines
        let mut new_lines: Vec<Line> = Vec::new();
        
        for (seg_idx, &is_deleted) in delete_segment.iter().enumerate() {
            if is_deleted {
                continue;
            }
            
            if seg_idx >= graph.segments.len() {
                continue;
            }
            
            let seg = &graph.segments[seg_idx];
            let a_idx = seg.a as usize;
            let b_idx = seg.b as usize;
            
            if a_idx >= graph.nodes.len() || b_idx >= graph.nodes.len() {
                continue;
            }
            
            let node_a = &graph.nodes[a_idx];
            let node_b = &graph.nodes[b_idx];
            
            new_lines.push(Line {
                x1: node_a.x,
                y1: node_a.y,
                x2: node_b.x,
                y2: node_b.y,
            });
        }
        
        // Update lines with trimmed segments
        self.lines = new_lines;
        
        // Push undo command
        self.history.push(Command::CleanOverhangs(previous_lines));
        
        // Refresh everything
        self.refresh_export();
        self.recompute_intersections();
        self.build_fill_graph();
    }
    fn undo(&mut self) {
        match self.history.pop() {
            Some(Command::Add) => {
                self.lines.pop();
            }
            Some(Command::AddFill) => {
                self.fills.pop();
            }
            Some(Command::AddFrame) => {
                // Remove last 4 lines (frame is always 4 lines added together)
                for _ in 0..4 {
                    self.lines.pop();
                }
            }
            Some(Command::Clear(previous_lines, previous_fills)) => {
                self.lines = previous_lines;
                self.fills = previous_fills;
            }
            Some(Command::CleanOverhangs(previous_lines)) => {
                self.lines = previous_lines;
            }
            None => {}
        }
        self.refresh_export();
        self.refresh_export_fills();
        self.recompute_intersections();
        self.build_fill_graph();
        
        // Verify state after undo in debug builds
        check_editor_integrity(self);
    }

    fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }

    fn export_ptr(&self) -> *const f32 {
        self.export_buf.as_ptr()
    }

    fn export_len(&self) -> u32 {
        self.export_buf.len() as u32
    }

    fn debug_ptr(&self) -> *const f32 {
        self.debug_buf.as_ptr()
    }

    fn debug_len(&self) -> u32 {
        self.debug_buf.len() as u32
    }

    fn intersections_ptr(&self) -> *const f32 {
        self.intersections_export.as_ptr()
    }

    fn intersections_len(&self) -> u32 {
        self.intersections_export.len() as u32
    }

    fn fill_trace_ptr(&self) -> *const f32 {
        self.fill_trace_buf.as_ptr()
    }

    fn fill_trace_len(&self) -> u32 {
        self.fill_trace_buf.len() as u32
    }

    fn fill_candidates_ptr(&self) -> *const f32 {
        self.fill_candidates_buf.as_ptr()
    }

    fn fill_candidates_len(&self) -> u32 {
        self.fill_candidates_buf.len() as u32
    }

    fn adjacency_debug_ptr(&self) -> *const f32 {
        self.adjacency_debug_buf.as_ptr()
    }

    fn adjacency_debug_len(&self) -> u32 {
        self.adjacency_debug_buf.len() as u32
    }

    fn node_outgoing_ptr(&self) -> *const f32 {
        self.node_outgoing_buf.as_ptr()
    }

    fn node_outgoing_len(&self) -> u32 {
        self.node_outgoing_buf.len() as u32
    }

    fn node_audit_ptr(&self) -> *const f32 {
        self.node_audit_buf.as_ptr()
    }

    fn node_audit_len(&self) -> u32 {
        self.node_audit_buf.len() as u32
    }

    fn graph_debug_ptr(&self) -> *const f32 {
        self.graph_debug_buf.as_ptr()
    }

    fn graph_debug_len(&self) -> u32 {
        self.graph_debug_buf.len() as u32
    }

    fn fill_walk_debug_ptr(&self) -> *const f32 {
        self.fill_walk_debug_buf.as_ptr()
    }

    fn fill_walk_debug_len(&self) -> u32 {
        self.fill_walk_debug_buf.len() as u32
    }
}

// LOCK POLICY:
// This global is accessed only from single-threaded WASM.
// JavaScript calls all editor_* functions from the main thread only.
// SAFETY: UnsafeCell is wrapped in Sync ONLY because we guarantee single-threaded access.
// If Web Workers or WASM threads are used, this MUST be changed to Mutex<Option<Editor>>.
struct EditorCell {
    inner: UnsafeCell<Option<Editor>>,
}

unsafe impl Sync for EditorCell {}  // Only safe for single-threaded WASM

static EDITOR: EditorCell = EditorCell {
    inner: UnsafeCell::new(None),
};

fn editor_mut() -> Option<&'static mut Editor> {
    unsafe { (&mut *EDITOR.inner.get()).as_mut() }
}

fn editor_ref() -> Option<&'static Editor> {
    unsafe { (&*EDITOR.inner.get()).as_ref() }
}

#[no_mangle]
pub extern "C" fn editor_init() {
    unsafe {
        *EDITOR.inner.get() = Some(Editor::new());
    }
}

#[no_mangle]
pub extern "C" fn editor_add_line(x1: f32, y1: f32, x2: f32, y2: f32) {
    if let Some(editor) = editor_mut() {
        editor.add_line(Line { x1, y1, x2, y2 });
    }
}

#[no_mangle]
pub extern "C" fn editor_add_frame(x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32) {
    if let Some(editor) = editor_mut() {
        editor.add_frame(x1, y1, x2, y2, x3, y3, x4, y4);
    }
}

#[no_mangle]
pub extern "C" fn editor_undo() {
    if let Some(editor) = editor_mut() {
        editor.undo();
    }
}

#[no_mangle]
pub extern "C" fn editor_clear() {
    if let Some(editor) = editor_mut() {
        editor.clear();
    }
}

#[no_mangle]
pub extern "C" fn editor_cleanup_overhangs() {
    if let Some(editor) = editor_mut() {
        editor.cleanup_overhangs();
    }
}

#[no_mangle]
pub extern "C" fn editor_line_count() -> u32 {
    editor_ref().map(|e| e.line_count()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_export_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.export_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_export_len_f32() -> u32 {
    editor_ref().map(|e| e.export_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_export_fills_ptr() -> *const f32 {
    if let Some(editor) = editor_mut() {
        editor.refresh_fills_export_buf();
        return editor.fills_export_buf.as_ptr();
    }
    ptr::null()
}

#[no_mangle]
pub extern "C" fn editor_export_fills_len() -> u32 {
    if let Some(editor) = editor_mut() {
        editor.refresh_fills_export_buf();
        return editor.fills_export_buf.len() as u32;
    }
    0
}

#[no_mangle]
pub extern "C" fn editor_set_debug(enabled: u32) {
    if let Some(editor) = editor_mut() {
        editor.debug_enabled = enabled != 0;
    }
}

#[no_mangle]
pub extern "C" fn editor_nearest(px: f32, py: f32) {
    if let Some(editor) = editor_mut() {
        if editor.debug_enabled {
            editor.compute_nearest_debug(px, py);
        } else {
            editor.debug_buf.clear();
            editor.debug_buf.push(0.0);
        }
    }
}

#[no_mangle]
pub extern "C" fn editor_debug_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.debug_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_debug_len_f32() -> u32 {
    editor_ref().map(|e| e.debug_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_intersections_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.intersections_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_intersections_len_f32() -> u32 {
    editor_ref().map(|e| e.intersections_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_fill(x: f32, y: f32, color_ptr: *const u8, color_len: usize) {
    if let Some(editor) = editor_mut() {
        let color_slice = unsafe { core::slice::from_raw_parts(color_ptr, color_len) };
        let color = parse_hex_color(color_slice);

        if let Some(nearest_idx) = find_nearest_line(&editor.lines, x, y, f32::INFINITY) {
            let connected = find_connected_lines(&editor.lines, nearest_idx);
            let polygon = trace_polygon(&editor.lines, &connected);

            if polygon.is_closed() {
                editor.fills.push(polygon.with_color(color));
                editor.history.push(Command::AddFill);
                editor.refresh_export_fills();
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn editor_fill_debug_at(ox: f32, oy: f32) {
    if let Some(editor) = editor_mut() {
        editor.fill_debug_at(ox, oy);
    }
}

#[no_mangle]
pub extern "C" fn editor_fills_count() -> u32 {
    editor_ref().map(|e| e.fills.len() as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_set_fill_color(color_ptr: *const u8, color_len: usize) {
    if let Some(editor) = editor_mut() {
        let color_slice = unsafe { core::slice::from_raw_parts(color_ptr, color_len) };
        editor.fill_color = parse_hex_color(color_slice);
    }
}

#[no_mangle]
pub extern "C" fn editor_fill_trace_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.fill_trace_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_fill_trace_len_f32() -> u32 {
    editor_ref().map(|e| e.fill_trace_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_fill_candidates_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.fill_candidates_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_fill_candidates_len_f32() -> u32 {
    editor_ref().map(|e| e.fill_candidates_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_adjacency_debug_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.adjacency_debug_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_adjacency_debug_len_f32() -> u32 {
    editor_ref().map(|e| e.adjacency_debug_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_node_outgoing_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.node_outgoing_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_node_outgoing_len_f32() -> u32 {
    editor_ref().map(|e| e.node_outgoing_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_node_audit_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.node_audit_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_node_audit_len_f32() -> u32 {
    editor_ref().map(|e| e.node_audit_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_export_graph_debug_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.graph_debug_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_export_graph_debug_len_f32() -> u32 {
    editor_ref().map(|e| e.graph_debug_len()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn editor_fill_walk_debug_ptr_f32() -> *const f32 {
    editor_ref()
        .map(|e| e.fill_walk_debug_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn editor_fill_walk_debug_len_f32() -> u32 {
    editor_ref().map(|e| e.fill_walk_debug_len()).unwrap_or(0)
}
