#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::f32::consts::PI;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::hint::spin_loop;

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
const TAU: f32 = PI * 2.0;
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
    Clear(Vec<Line>),
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

        let mut registry = IntersectionRegistry::new();
        let mut per_line_nodes: Vec<Vec<(f32, u32)>> = Vec::new();
        per_line_nodes.resize(self.lines.len(), Vec::new());

        // Insert endpoints
        for (idx, line) in self.lines.iter().enumerate() {
            let n0 = registry.get_or_insert(line.x1, line.y1, &mut self.fill_graph.nodes);
            let n1 = registry.get_or_insert(line.x2, line.y2, &mut self.fill_graph.nodes);
            per_line_nodes[idx].push((0.0, n0));
            per_line_nodes[idx].push((1.0, n1));
        }

        // Collect intersections with shared node ids
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

    fn fill_debug_at(&mut self, ox: f32, oy: f32) {
        self.fill_trace_buf.clear();
        self.fill_candidates_buf.clear();
        self.node_outgoing_buf.clear();
        self.fill_walk_debug_buf.clear();
        self.build_fill_graph();

        // Step 0: origin
        self.fill_trace_buf.push(ox);
        self.fill_trace_buf.push(oy);
        self.fill_trace_buf.push(0.0); // type = origin

        if self.fill_graph.segments.is_empty() {
            let mut result = Vec::new();
            result.push((self.fill_trace_buf.len() / 3) as f32);
            result.extend(self.fill_trace_buf.drain(..));
            self.fill_trace_buf = result;
            return;
        }

        // Pick starting sector nearest to origin (midpoint distance)
        let mut best_seg_idx = 0usize;
        let mut best_dist2 = f32::INFINITY;
        for (i, seg) in self.fill_graph.segments.iter().enumerate() {
            let a = self.fill_graph.nodes[seg.a as usize];
            let b = self.fill_graph.nodes[seg.b as usize];
            let mid_x = (a.x + b.x) * 0.5;
            let mid_y = (a.y + b.y) * 0.5;
            let d2 = distance_sq(ox, oy, mid_x, mid_y);
            if d2 < best_dist2 {
                best_dist2 = d2;
                best_seg_idx = i;
            }
        }

        let start_seg = self.fill_graph.segments[best_seg_idx];
        let pa = self.fill_graph.nodes[start_seg.a as usize];
        let pb = self.fill_graph.nodes[start_seg.b as usize];
        let dist_a = distance_sq(ox, oy, pa.x, pa.y);
        let dist_b = distance_sq(ox, oy, pb.x, pb.y);

        let mut cur_node = if dist_a <= dist_b { start_seg.a } else { start_seg.b };
        let mut prev_node = if cur_node == start_seg.a { start_seg.b } else { start_seg.a };
        let mut cur_sector = best_seg_idx as u32;
        let start_prev = prev_node;
        let start_cur = cur_node;

        // Trace start node
        let cur_pt = self.fill_graph.nodes[cur_node as usize];
        self.fill_trace_buf.push(cur_pt.x);
        self.fill_trace_buf.push(cur_pt.y);
        self.fill_trace_buf.push(2.0); // start node

        let mut visited_edges: Vec<(u32, u32)> = Vec::new();
        visited_edges.push((prev_node, cur_node));

        let max_steps = (self.fill_graph.segments.len() as u32) * 2 + 2;
        let mut step_idx = 0u32;

        loop {
            if step_idx >= max_steps {
                // step limit
                let pt = self.fill_graph.nodes[cur_node as usize];
                self.fill_trace_buf.push(pt.x);
                self.fill_trace_buf.push(pt.y);
                self.fill_trace_buf.push(7.0); // step limit
                break;
            }

            let cur_pt = self.fill_graph.nodes[cur_node as usize];
            let prev_pt = self.fill_graph.nodes[prev_node as usize];
            let vin = (cur_pt.x - prev_pt.x, cur_pt.y - prev_pt.y);
            let (vinx, viny, vinlen) = normalize_with_len(vin.0, vin.1);

            // Candidate sectors: those attached to cur_node excluding the sector we came from
            let mut step_block: Vec<f32> = Vec::new();
            let cur_deg = if (cur_node as usize) < self.node_degree.len() { self.node_degree[cur_node as usize] } else { 0 };

            let sectors_here = if (cur_node as usize) < self.fill_graph.node_sectors.len() {
                self.fill_graph.node_sectors[cur_node as usize].clone()
            } else {
                Vec::new()
            };

            // Compute turn angle for each candidate: smallest positive turn = keep-left rule
            // This ensures consistent CCW boundary following
            let mut candidates_info: Vec<(u32, u32, u32, bool, f32)> = Vec::new(); // (sector_id, next_node, next_deg, is_dead, turn_angle)
            for sid in sectors_here.iter() {
                if *sid == cur_sector {
                    continue;
                }
                let seg = self.fill_graph.segments[*sid as usize];
                let next_node = other_end(&seg, cur_node);
                let next_deg_total = if (next_node as usize) < self.node_degree.len() {
                    self.node_degree[next_node as usize]
                } else { 0 };

                let attached = if (next_node as usize) < self.fill_graph.node_sectors.len() {
                    self.fill_graph.node_sectors[next_node as usize].len()
                } else { 0 };
                let cnt_without_back = if attached > 0 { attached - 1 } else { 0 };
                let is_dead = next_deg_total <= 1 || cnt_without_back == 0;

                let next_pt = self.fill_graph.nodes[next_node as usize];
                let (voutx, vouty, voutlen) = normalize_with_len(next_pt.x - cur_pt.x, next_pt.y - cur_pt.y);
                
                // Compute signed turn angle from incoming to outgoing vector
                // turn = atan2(cross(vin,vout), dot(vin,vout)) normalized to [0, 2π)
                // Smallest positive turn = keep-left rule for CCW boundary traversal
                let angle = if vinlen < 1e-6 || voutlen < 1e-6 {
                    0.0
                } else {
                    let dotp = vinx * voutx + viny * vouty;
                    let crossp = vinx * vouty - viny * voutx;
                    let mut a = atan2_approx(crossp, dotp);
                    if a < 0.0 { a += TAU; }
                    a
                };

                candidates_info.push((*sid, next_node, next_deg_total, is_dead, angle));
            }

            // Choose candidate: filter dead-ends, then pick smallest turn angle
            // Tie-break: smallest next node id for determinism
            let mut usable: Vec<(u32, u32, u32, bool, f32)> = Vec::new();
            for c in candidates_info.iter() {
                if !c.3 {
                    usable.push(*c);
                }
            }

            let mut chosen: Option<(u32, u32)> = None; // (sector, next_node)
            if usable.len() == 1 {
                chosen = Some((usable[0].0, usable[0].1));
            } else if usable.len() > 1 {
                usable.sort_by(|a, b| {
                    if absf(a.4 - b.4) > 1e-6 {
                        if a.4 < b.4 { core::cmp::Ordering::Less } else { core::cmp::Ordering::Greater }
                    } else {
                        a.1.cmp(&b.1)
                    }
                });
                chosen = Some((usable[0].0, usable[0].1));
            }

            // Log step
            step_block.push(step_idx as f32);
            step_block.push(cur_pt.x);
            step_block.push(cur_pt.y);
            step_block.push(prev_pt.x);
            step_block.push(prev_pt.y);
            step_block.push(cur_deg as f32);
            step_block.push(candidates_info.len() as f32);

            for c in candidates_info.iter() {
                let next_pt = self.fill_graph.nodes[c.1 as usize];
                let is_chosen = if let Some((sid, _)) = chosen { sid == c.0 } else { false };
                step_block.push(next_pt.x);
                step_block.push(next_pt.y);
                step_block.push(c.2 as f32);
                step_block.push(if c.3 { 1.0 } else { 0.0 });
                step_block.push(c.4);
                step_block.push(if is_chosen { 1.0 } else { 0.0 });
            }

            self.fill_walk_debug_buf.extend(step_block.into_iter());

            if chosen.is_none() {
                // Dead end: no non-dead candidates
                self.fill_trace_buf.push(cur_pt.x);
                self.fill_trace_buf.push(cur_pt.y);
                self.fill_trace_buf.push(8.0);
                break;
            }

            let (chosen_sector, next_node) = chosen.unwrap();

            // Cycle detection: stop if we've traversed this directed edge before
            // This means we've completed a closed face boundary
            let edge = (cur_node, next_node);
            let mut seen = false;
            for e in visited_edges.iter() {
                if *e == edge {
                    seen = true;
                    break;
                }
            }
            if seen || (cur_node == start_prev && next_node == start_cur) {
                let pt = self.fill_graph.nodes[next_node as usize];
                self.fill_trace_buf.push(pt.x);
                self.fill_trace_buf.push(pt.y);
                self.fill_trace_buf.push(9.0);
                
                // Success! Create a polygon from the traced path
                self.create_polygon_from_trace();
                break;
            }

            visited_edges.push(edge);

            let next_pt = self.fill_graph.nodes[next_node as usize];
            self.fill_trace_buf.push(next_pt.x);
            self.fill_trace_buf.push(next_pt.y);
            self.fill_trace_buf.push(3.0);

            prev_node = cur_node;
            cur_node = next_node;
            cur_sector = chosen_sector;
            step_idx += 1;
        }

        // Prepend count
        let mut result = Vec::new();
        result.push((self.fill_trace_buf.len() / 3) as f32);
        result.extend(self.fill_trace_buf.drain(..));
        self.fill_trace_buf = result;
    }

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

    fn clear(&mut self) {
        let previous = self.lines.clone();
        self.lines.clear();
        self.fills.clear();
        self.history.push(Command::Clear(previous));
        self.fill_trace_buf.clear();
        self.refresh_export();
        self.recompute_intersections();
        self.build_fill_graph();
    }

    fn undo(&mut self) {
        match self.history.pop() {
            Some(Command::Add) => {
                self.lines.pop();
            }
            Some(Command::Clear(previous)) => {
                self.lines = previous;
            }
            None => {}
        }
        self.refresh_export();
        self.recompute_intersections();
        self.build_fill_graph();
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

struct EditorCell {
    inner: UnsafeCell<Option<Editor>>,
}

unsafe impl Sync for EditorCell {}

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
