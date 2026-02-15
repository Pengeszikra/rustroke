# Rustroke: Integrity & Concurrency Safety

## Lock Policy & Thread Safety

### Current Architecture
- **Single-threaded WASM** execution model
- Global `EDITOR` state wrapped in `UnsafeCell` (no locks)
- JavaScript calls from browser main thread only

### Safety Guarantees
1. **Compile-time check**: Code will NOT compile with WASM atomics/threads enabled
2. **Runtime assumption**: All `editor_*` exports called from single thread
3. **No data races**: UnsafeCell safe ONLY because of single-threaded guarantee

### Lock Policy (for future threading)

```
Rule 1: Current State (Single-threaded)
  - All editor_* functions assume single-threaded access
  - Never call from Web Worker without synchronization
  - Compile-time error if atomics enabled

Rule 2: If Threading Added Later
  - Change: static EDITOR: Mutex<Option<Editor>>
  - Lock order: ALWAYS acquire EDITOR lock first, then any sub-locks
  - Lock scope: lock → copy data → unlock → compute
  - NEVER hold lock while: rendering, IO, JS callbacks, re-entrant calls

Rule 3: Deadlock Prevention
  - Single global lock (Mutex<Editor>) - simplest approach
  - Alternative: Use mpsc channel for commands (single worker thread)
  - NO nested locks (if multiple needed, acquire by address order)
  - Keep critical sections < 1ms
```

### Migration Path to Threading

If Web Workers or WASM threads are needed:

```rust
// BEFORE (current - single-threaded)
static EDITOR: EditorCell = EditorCell {
    inner: UnsafeCell::new(None),
};

// AFTER (thread-safe)
use std::sync::Mutex;
static EDITOR: Mutex<Option<Editor>> = Mutex::new(None);

fn editor_mut() -> Option<MutexGuard<'static, Option<Editor>>> {
    EDITOR.lock().ok()
}
```

---

## Integrity Checks (Debug Builds Only)

### Overview
All checks are `#[cfg(debug_assertions)]` - **zero overhead in release**.

### What's Checked

#### 1. Coordinate Validation
- **Where**: Every line add, fill operation
- **Checks**: All coordinates are finite (not NaN/Inf)
- **Why**: Corrupt coordinates crash rendering and break intersection math

```rust
check_line_coordinates(x1, y1, x2, y2);
// Asserts: x1.is_finite() && y1.is_finite() && ...
```

#### 2. Graph Store (DSU) Integrity
- **Where**: After undo, after cleanup_overhangs
- **Checks**:
  - DSU parent pointers valid (parent[root] == root)
  - Component sizes non-zero and consistent
  - `odd_count` matches actual odd-degree nodes
  - Edge endpoints are valid node IDs

```rust
graph.check_dsu_integrity();       // Parent pointers
graph.check_component_parity();    // Odd count accuracy
```

#### 3. Fill Graph Structure
- **Where**: After build_fill_graph()
- **Checks**:
  - All nodes have finite coordinates
  - Segments reference valid nodes
  - No degenerate segments (a == b)
  - Half-edges reference valid nodes and segments
  - Outgoing lists reference valid half-edges

```rust
check_fill_graph_integrity(&fill_graph);
```

#### 4. Editor State Consistency
- **Where**: After undo
- **Checks**:
  - All lines have finite coordinates
  - All fill polygons have finite points
  - Graph store integrity
  - Fill graph integrity

```rust
check_editor_integrity(editor);
// Runs all sub-checks
```

### Performance Impact

| Build Type | Runtime Cost | Binary Size |
|------------|--------------|-------------|
| Debug      | ~1-5% slower | +20KB       |
| Release    | **0%**       | **0 bytes** |

All checks compile to nothing in `--release` builds.

### Testing the Checks

To verify checks work, force a debug build:

```bash
# Debug build (checks enabled)
cargo +nightly build --target wasm32-unknown-unknown

# This will be slower but catch bugs early
```

---

## Manual Test Checklist

### Basic Integrity
- [ ] Draw line with valid coordinates → works
- [ ] Draw line, undo → no assertion failures
- [ ] Clear canvas → no assertion failures

### Graph Integrity
- [ ] Draw closed triangle → fills correctly
- [ ] Draw open line → no fill (component not closed)
- [ ] Draw triangle + dangling line → only triangle fills
- [ ] Trim overhangs → no assertion failures

### Stress Tests
- [ ] **Rapid draw test**: Hold mouse, draw 100+ lines quickly
- [ ] **Rapid undo test**: Add 50 lines, undo all rapidly
- [ ] **Fill spam test**: Draw triangle, click fill 20 times rapidly
- [ ] **Mixed operations**: Draw + Fill + Undo + Trim in random order

### Edge Cases
- [ ] Draw zero-length line (x1==x2, y1==y2) → rejected
- [ ] Fill on empty canvas → no crash
- [ ] Undo on empty history → no crash
- [ ] Trim overhangs on closed shape → shape preserved

### Debug Build Validation
If running debug build, intentionally corrupt state:

```rust
// In debug_checks.rs test:
editor.lines.push(Line { 
    x1: f32::NAN, y1: 0.0, 
    x2: 0.0, y2: 0.0 
});
check_editor_integrity(editor);
// Should panic: "Line has non-finite coordinates"
```

---

## Files Modified

1. **src/lib.rs**
   - Added compile-time thread-safety check
   - Added `mod debug_checks`
   - Added check calls in: `add_line`, `clear`, `undo`, `fill_debug_at`, `build_fill_graph`
   - Documented lock policy in comments

2. **src/graph.rs**
   - Added `nodes_iter()`, `edges_iter()` for debug access
   - Added `check_dsu_integrity()` - validates parent pointers
   - Added `check_component_parity()` - validates odd_count tracking

3. **src/debug_checks.rs** (NEW)
   - `check_line_coordinates()` - finite coordinate check
   - `check_graph_integrity()` - DSU and component checks
   - `check_fill_graph_integrity()` - fill graph structure checks
   - `check_editor_integrity()` - full editor state validation

---

## Risk Mitigation Summary

| Risk                          | Before | After  | Mitigation                          |
|-------------------------------|--------|--------|-------------------------------------|
| Thread safety                 | ❌     | ✅     | Compile-time check + docs           |
| NaN/Inf coordinates           | ❌     | ✅     | Debug assertions on all inputs      |
| Invalid DSU parent pointers   | ❌     | ✅     | check_dsu_integrity()               |
| Component odd_count mismatch  | ❌     | ✅     | check_component_parity()            |
| Invalid node/segment indices  | ❌     | ✅     | Range checks in fill graph          |
| Corrupted undo state          | ❌     | ✅     | check_editor_integrity() after undo |
| Degenerate fill graph         | ❌     | ✅     | Segment validation (a != b)         |

**Result**: Early detection of state corruption in development, zero overhead in production.

---

## Future Improvements (If Needed)

### If Memory Becomes Constrained
- Track allocation high-water mark
- Add `heap_usage()` export for monitoring
- Warn when > 80% heap used

### If Performance Becomes Issue
- Profile with `#[no_mangle]` exports for timing
- Add debug-only performance counters
- Log slow operations (> 16ms frame time)

### If Threading Added
1. Replace `UnsafeCell` with `Mutex`
2. Add `try_lock()` with timeout
3. Use `mpsc` channel for commands (simpler than multiple locks)
4. Consider lock-free ring buffer for high-freq updates
