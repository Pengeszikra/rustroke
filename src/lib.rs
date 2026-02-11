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
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        spin_loop();
    }
}

// -------- Editor core --------
#[derive(Clone, Copy)]
struct Line {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

enum Command {
    Add,
    Clear(Vec<Line>),
}

struct Editor {
    lines: Vec<Line>,
    history: Vec<Command>,
    export_buf: Vec<f32>,
}

impl Editor {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            history: Vec::new(),
            export_buf: Vec::new(),
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
