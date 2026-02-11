#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::hint::spin_loop;

// -------- Minimal allocator --------
const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB bump heap

struct BumpAllocator {
    offset: AtomicUsize,
}

struct Heap {
    buf: UnsafeCell<[u8; HEAP_SIZE]>,
}

// Safe because we only mutate through atomic bump offsets, never concurrently.
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
                // Out of memory: trap by spinning forever
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

    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        // no-op; bump allocator never reclaims
    }
}

#[global_allocator]
static GLOBAL: BumpAllocator = BumpAllocator {
    offset: AtomicUsize::new(0),
};

#[panic_handler]
#[cfg(target_arch = "wasm32")]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        spin_loop();
    }
}

// -------- Editor core --------
const TOLERANCE: f32 = 10.0;

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

// Find intersection point of two line segments
fn line_intersection(x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, x4: f32, y4: f32) -> Option<(f32, f32)> {
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if denom.abs() < 0.0001 {
        return None; // Lines are parallel
    }

    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
    let u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / denom;

    // Check if intersection is within both segments
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

fn find_connected_lines(lines: &[Line], line_idx: usize) -> Vec<usize> {
    let mut connected = Vec::new();
    connected.push(line_idx);

    let tolerance_sq = TOLERANCE * TOLERANCE;
    let start_line = lines[line_idx];
    let mut current_end = (start_line.x2, start_line.y2);
    let mut seen: [bool; 256] = [false; 256];
    if line_idx < 256 {
        seen[line_idx] = true;
    }

    while connected.len() < lines.len() {
        if let Some(next_idx) = find_nearest_line(lines, current_end.0, current_end.1, tolerance_sq) {
            if next_idx < 256 && seen[next_idx] {
                break;
            }
            let next_line = lines[next_idx];
            let dist_start = distance_sq(current_end.0, current_end.1, next_line.x1, next_line.y1);
            let dist_end = distance_sq(current_end.0, current_end.1, next_line.x2, next_line.y2);

            if dist_start < dist_end {
                current_end = (next_line.x2, next_line.y2);
            } else {
                current_end = (next_line.x1, next_line.y1);
            }

            connected.push(next_idx);
            if next_idx < 256 {
                seen[next_idx] = true;
            }

            let start_dist = distance_sq(
                current_end.0,
                current_end.1,
                start_line.x1,
                start_line.y1,
            );
            if start_dist < tolerance_sq {
                break;
            }
        } else {
            break;
        }
    }

    connected
}

fn trace_polygon(lines: &[Line], line_indices: &[usize]) -> Polygon {
    let mut polygon = Polygon::new();

    if line_indices.is_empty() {
        return polygon;
    }

    let first_line = lines[line_indices[0]];
    polygon.push(first_line.x1, first_line.y1);

    for &idx in line_indices.iter() {
        let line = lines[idx];
        polygon.push(line.x2, line.y2);
    }

    polygon
}

struct Editor {
    lines: Vec<Line>,
    history: Vec<Command>,
    export_buf: Vec<f32>,
    fills: Vec<Polygon>,
}

impl Editor {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            history: Vec::new(),
            export_buf: Vec::new(),
            fills: Vec::new(),
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

    fn add_line(&mut self, line: Line) {
        self.lines.push(line);
        self.history.push(Command::Add);
        self.refresh_export();
    }

    fn clear(&mut self) {
        let previous = self.lines.clone();
        self.lines.clear();
        self.fills.clear();
        self.history.push(Command::Clear(previous));
        self.refresh_export();
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
        self.fills.clear();
        self.refresh_export();
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

    fn fills_len(&self) -> u32 {
        let mut total = 0;
        for polygon in self.fills.iter() {
            total += 1 + polygon.points.len() * 2;
        }
        total as u32
    }

    fn export_fills(&mut self) -> Vec<f32> {
        let mut result: Vec<f32> = Vec::new();
        for polygon in self.fills.iter() {
            result.push(polygon.points.len() as f32);
            // Color components normalized to 0-1
            result.push((polygon.color >> 24) as u8 as f32 / 255.0);
            result.push(((polygon.color >> 16) & 0xFF) as u8 as f32 / 255.0);
            result.push(((polygon.color >> 8) & 0xFF) as u8 as f32 / 255.0);
            result.push((polygon.color & 0xFF) as u8 as f32 / 255.0);
            // Points
            for (x, y) in polygon.points.iter() {
                result.push(*x);
                result.push(*y);
            }
        }
        result
    }
}

struct EditorCell {
    inner: UnsafeCell<Option<Editor>>,
}

// Safe because the JS/WASM host is single-threaded in this example.
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
pub extern "C" fn editor_fill(x: f32, y: f32, color_ptr: *const u8, color_len: usize) {
    if let Some(editor) = editor_mut() {
        let color_slice = unsafe { core::slice::from_raw_parts(color_ptr, color_len) };
        let color = parse_hex_color(color_slice);

        // Debug: find and log all intersection points
        let mut intersections = Vec::new();
        for i in 0..editor.lines.len() {
            for j in (i + 1)..editor.lines.len() {
                let l1 = &editor.lines[i];
                let l2 = &editor.lines[j];
                if let Some((ix, iy)) = line_intersection(l1.x1, l1.y1, l1.x2, l1.y2, l2.x1, l2.y1, l2.x2, l2.y2) {
                    intersections.push((ix, iy));
                }
            }
        }

        // Store intersection points in export buffer for JS to log
        editor.export_buf.clear();
        editor.export_buf.push(intersections.len() as f32);
        for (ix, iy) in intersections {
            editor.export_buf.push(ix);
            editor.export_buf.push(iy);
        }

        // Find nearest line to click point
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
pub extern "C" fn editor_export_fills_len() -> u32 {
    if let Some(editor) = editor_ref() {
        editor.fills_len()
    } else {
        0
    }
}

static mut FILLS_EXPORT_BUF: Option<Vec<f32>> = None;

#[no_mangle]
pub extern "C" fn editor_export_fills_ptr() -> *const f32 {
    unsafe {
        if let Some(editor) = editor_mut() {
            let buf = editor.export_fills();
            FILLS_EXPORT_BUF = Some(buf);
            if let Some(ref buf) = FILLS_EXPORT_BUF {
                return buf.as_ptr();
            }
        }
        ptr::null()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn test_distance_sq() {
        assert_eq!(distance_sq(0.0, 0.0, 3.0, 4.0), 25.0);
        assert_eq!(distance_sq(0.0, 0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn test_polygon_creation() {
        let mut poly = Polygon::new();
        assert!(!poly.is_closed());

        poly.push(0.0, 0.0);
        poly.push(10.0, 0.0);
        poly.push(10.0, 10.0);
        assert!(poly.is_closed());
        assert_eq!(poly.points.len(), 3);
    }

    #[test]
    fn test_find_nearest_line() {
        let lines: [Line; 3] = [
            Line {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 0.0,
            },
            Line {
                x1: 20.0,
                y1: 20.0,
                x2: 30.0,
                y2: 30.0,
            },
            Line {
                x1: 50.0,
                y1: 50.0,
                x2: 60.0,
                y2: 60.0,
            },
        ];

        let lines_vec: Vec<Line> = {
            let mut v = Vec::new();
            for line in lines.iter() {
                v.push(*line);
            }
            v
        };

        let nearest = find_nearest_line(&lines_vec, 0.0, 0.0, 100.0);
        assert_eq!(nearest, Some(0));

        let nearest = find_nearest_line(&lines_vec, 25.0, 25.0, 100.0);
        assert_eq!(nearest, Some(1));
    }

    #[test]
    fn test_trace_polygon() {
        let lines: [Line; 3] = [
            Line {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 0.0,
            },
            Line {
                x1: 10.0,
                y1: 0.0,
                x2: 10.0,
                y2: 10.0,
            },
            Line {
                x1: 10.0,
                y1: 10.0,
                x2: 0.0,
                y2: 0.0,
            },
        ];

        let lines_vec: Vec<Line> = {
            let mut v = Vec::new();
            for line in lines.iter() {
                v.push(*line);
            }
            v
        };

        let indices: [usize; 3] = [0, 1, 2];
        let indices_vec: Vec<usize> = {
            let mut v = Vec::new();
            for idx in indices.iter() {
                v.push(*idx);
            }
            v
        };

        let poly = trace_polygon(&lines_vec, &indices_vec);
        assert_eq!(poly.points.len(), 4);
    }

    #[test]
    fn test_tolerance_is_10() {
        assert_eq!(TOLERANCE, 10.0);
    }
}
