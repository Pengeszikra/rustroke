#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
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

fn floor_f32(x: f32) -> f32 {
    let truncated = x as i32 as f32;
    if truncated <= x {
        truncated
    } else {
        truncated - 1.0
    }
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
// Returns Some((ix, iy)) if they intersect within [0,1] on both
#[allow(dead_code)]
fn line_intersection(x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32) -> Option<(f32, f32)> {
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if denom.abs() < 0.0001 {
        return None;
    }

    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
    let u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / denom;

    if t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0 {
        let ix = x1 + t * (x2 - x1);
        let iy = y1 + t * (y2 - y1);
        return Some((ix, iy));
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
                    ((current_line.x1 - other_line.x1).abs() + (current_line.y1 - other_line.y1).abs() < TOLERANCE) ||
                    ((current_line.x1 - other_line.x2).abs() + (current_line.y1 - other_line.y2).abs() < TOLERANCE) ||
                    ((current_line.x2 - other_line.x1).abs() + (current_line.y2 - other_line.y1).abs() < TOLERANCE) ||
                    ((current_line.x2 - other_line.x2).abs() + (current_line.y2 - other_line.y2).abs() < TOLERANCE);

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

            let dx1 = (current_end.0 - line.x1).abs();
            let dy1 = (current_end.1 - line.y1).abs();
            let dx2 = (current_end.0 - line.x2).abs();
            let dy2 = (current_end.1 - line.y2).abs();

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
        }
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

    // Recompute and cache intersection points whenever geometry changes
    fn recompute_intersections(&mut self) {
        self.intersections.clear();

        // Find all intersection points
        for i in 0..self.lines.len() {
            for j in (i + 1)..self.lines.len() {
                let l1 = &self.lines[i];
                let l2 = &self.lines[j];
                if let Some((ix, iy)) = line_intersection(l1.x1, l1.y1, l1.x2, l1.y2, l2.x1, l2.y1, l2.x2, l2.y2) {
                    self.intersections.push((ix, iy));
                }
            }
        }

        // Deduplicate using epsilon snap grid
        if !self.intersections.is_empty() {
            // Snap to grid
            for point in self.intersections.iter_mut() {
                point.0 = floor_f32(point.0 / SNAP_EPS) * SNAP_EPS;
                point.1 = floor_f32(point.1 / SNAP_EPS) * SNAP_EPS;
            }

            // Sort deterministically
            let mut sorted = self.intersections.clone();
            sorted.sort_by(|a, b| {
                if (a.0 - b.0).abs() > 0.0001 {
                    if a.0 < b.0 { core::cmp::Ordering::Less } else { core::cmp::Ordering::Greater }
                } else {
                    if a.1 < b.1 { core::cmp::Ordering::Less } else if a.1 > b.1 { core::cmp::Ordering::Greater } else { core::cmp::Ordering::Equal }
                }
            });

            // Deduplicate
            self.intersections.clear();
            for point in sorted.iter() {
                if self.intersections.is_empty() || 
                   ((self.intersections[self.intersections.len() - 1].0 - point.0).abs() > 0.0001 ||
                    (self.intersections[self.intersections.len() - 1].1 - point.1).abs() > 0.0001) {
                    self.intersections.push(*point);
                }
            }
        }

        // Rebuild export buffer: [count, x1, y1, x2, y2, ...]
        self.intersections_export.clear();
        self.intersections_export.push(self.intersections.len() as f32);
        for (x, y) in self.intersections.iter() {
            self.intersections_export.push(*x);
            self.intersections_export.push(*y);
        }
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
        self.lines.push(line);
        self.history.push(Command::Add);
        self.refresh_export();
        self.recompute_intersections();
    }

    fn clear(&mut self) {
        let previous = self.lines.clone();
        self.lines.clear();
        self.fills.clear();
        self.history.push(Command::Clear(previous));
        self.refresh_export();
        self.recompute_intersections();
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
        let buf = editor.export_fills();
        return buf.as_ptr();
    }
    ptr::null()
}

#[no_mangle]
pub extern "C" fn editor_export_fills_len() -> u32 {
    if let Some(editor) = editor_mut() {
        let buf = editor.export_fills();
        return buf.len() as u32;
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
