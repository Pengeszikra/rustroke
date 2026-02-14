# Polyline Drawing Mode

## Overview

Rustroke now supports **two drawing modes**:
- **Freehand** (default): Draw single straight lines with pointer drag
- **Polyline**: Draw continuous straight-line chains (multiple connected segments)

## User Interface

### Mode Toggle Button
- **Location**: First button in toolbar (left side)
- **Label**: "Freehand" (default) or "Polyline" (when active)
- **Behavior**: Click to toggle between modes
- **Visual**: Button highlights when in Polyline mode

### Drawing in Polyline Mode

1. **Click "Freehand"** button to switch to **"Polyline"** mode
2. **Click** on canvas to start polyline
3. **Move** mouse - segments are committed automatically when you move far enough
4. **Click** again to end polyline and finalize all segments
5. All segments are added as one undo operation

### Preview

While drawing in Polyline mode:
- **White polyline**: Shows committed anchor points
- **White preview line**: Shows live cursor position from last anchor
- Both update smoothly as you draw

## Technical Details

### Parameters

```rust
MIN_SEG_LEN = 8.0    // Minimum distance (px) to commit new segment
ANGLE_EPS = 7.0      // Angle threshold (degrees) for merging collinear segments
```

### Segment Commit Rules

A new anchor point is committed when:
1. Distance from last anchor ≥ 8 pixels
2. If nearly collinear with previous segment (angle < 7°), replaces last anchor instead

### Angle Simplification

The algorithm automatically merges nearly straight segments:
- Uses normalized dot product: `cos(angle) = dot / (len1 * len2)`
- For 7°: `cos(7°) ≈ 0.993`
- If `dot_normalized > 0.993`, segments are collinear → merge

This reduces vertex count for smoother drawing.

### Data Flow

```
User draws → WASM polyline state → JS preview → User ends → WASM converts to lines
                                                                      ↓
                                                              Undo: single operation
```

## Implementation

### Rust (src/lib.rs)

**New Structures:**
```rust
struct PolylineState {
    anchors: Vec<Point>,     // Committed anchor points
    min_seg_len: f32,        // 8.0
    angle_eps: f32,          // 7.0 (unused, hardcoded as 0.993)
}

enum Command {
    ...
    AddPolyline(u32),  // Number of segments (for undo)
}
```

**Key Methods:**
```rust
// Start new polyline
fn polyline_start(&mut self, x: f32, y: f32)

// Update with cursor position, commit if threshold met
fn polyline_update(&mut self, x: f32, y: f32)

// Finalize and convert anchors to line segments
fn polyline_end(&mut self, x: f32, y: f32) -> u32

// Export preview data for JS rendering
fn export_polyline_preview(&mut self)
```

**WASM Exports:**
```rust
#[no_mangle]
pub extern "C" fn editor_polyline_start(x: f32, y: f32)

#[no_mangle]
pub extern "C" fn editor_polyline_update(x: f32, y: f32)

#[no_mangle]
pub extern "C" fn editor_polyline_end(x: f32, y: f32) -> u32

#[no_mangle]
pub extern "C" fn editor_polyline_preview_ptr() -> *const f32

#[no_mangle]
pub extern "C" fn editor_polyline_preview_len() -> u32
```

### JavaScript (web/main.js)

**State:**
```javascript
let toolMode = 'freehand';  // 'freehand' or 'polyline'
```

**Mode Toggle:**
```javascript
modeBtn.addEventListener('click', () => {
  toolMode = toolMode === 'freehand' ? 'polyline' : 'freehand';
  modeBtn.textContent = toolMode === 'freehand' ? 'Freehand' : 'Polyline';
  modeBtn.classList.toggle('active');
});
```

**Pointer Events:**
- `pointerdown`: Start polyline or single line based on mode
- `pointermove`: Update polyline or preview line
- `pointerup`: End polyline or add single line

**Preview Rendering:**
```javascript
function updatePolylinePreview() {
  const len = wasm.editor_polyline_preview_len();
  const ptr = wasm.editor_polyline_preview_ptr();
  const coords = new Float32Array(wasm.memory.buffer, ptr, len);
  
  const pointsStr = [];
  for (let i = 0; i < len; i += 2) {
    pointsStr.push(`${coords[i]},${coords[i+1]}`);
  }
  
  polylinePreview.setAttribute('points', pointsStr.join(' '));
}
```

### HTML (web/index.html)

**Mode Button:**
```html
<button id="modeBtn" title="Toggle drawing mode: Freehand / Polyline">Freehand</button>
```

**Polyline Preview Element:**
```html
<polyline id="polylinePreview" fill="none" stroke="white" stroke-width="1" pointer-events="none"></polyline>
```

## Performance

- **O(1) per pointer move**: No expensive computations during drawing
- **Angle check**: Simple dot product, no trigonometric functions
- **Memory**: Small vector of anchor points (typically < 100 points)
- **WASM size**: +2KB (67KB → 69KB)

## Undo Behavior

- **Freehand**: Each line = 1 undo operation
- **Polyline**: Entire polyline (all segments) = 1 undo operation

Example:
```
Draw polyline with 5 segments → Creates 5 lines
Press Undo → Removes all 5 lines at once
```

## Testing

### Automated Test
Open `http://localhost:8080/test-polyline.html`:
- Tests polyline start/update/end
- Verifies distance threshold
- Checks undo behavior
- Tests multiple polylines

### Manual Test
1. Switch to Polyline mode
2. Draw a zigzag pattern with 5-6 segments
3. Verify smooth preview updates
4. Verify all segments appear after release
5. Verify single undo removes entire polyline
6. Switch back to Freehand mode
7. Draw single line - verify it still works

## Known Limitations

- **No polyline recording**: Polylines are not yet supported in playback system
  (Each segment is stored as a separate line, so playback will work but won't show the original drawing flow)
- **No snap to grid**: Polyline anchors don't snap to existing nodes (future enhancement)

## Future Enhancements

1. **Snap to existing nodes**: Auto-connect to nearby endpoints
2. **Backspace to remove last anchor**: Allow correcting mistakes mid-draw
3. **Double-click to end**: Alternative to pointer-up
4. **Curve smoothing**: Optional Catmull-Rom spline interpolation
5. **Recording support**: Store polyline as single operation with all anchors
6. **Visual feedback**: Show anchor points as small circles during drawing

## File Changes

### Modified Files
- `src/lib.rs` (82 lines added)
  - PolylineState struct and methods
  - Editor polyline methods
  - WASM exports
  - Command::AddPolyline undo support

- `web/main.js` (95 lines modified)
  - toolMode state variable
  - Mode toggle button handler
  - Pointer event routing based on mode
  - Polyline preview rendering

- `web/index.html` (2 lines added)
  - Mode toggle button
  - Polyline preview SVG element

### New Files
- `web/test-polyline.html` (147 lines)
  - Automated polyline mode tests

- `POLYLINE_MODE.md` (this file)
  - Complete documentation

## Code Quality

- ✅ No external dependencies
- ✅ Deterministic behavior (no randomness)
- ✅ Fast: O(1) per move operation
- ✅ Memory safe: bounds-checked vectors
- ✅ Minimal diff: ~180 lines total added
- ✅ Backward compatible: Freehand mode unchanged
- ✅ Single warning: unused `angle_eps` field (hardcoded value used)

## Summary

Polyline mode provides a smooth, predictable way to draw multi-segment straight lines with:
- Automatic segment commitment based on distance
- Angle simplification to reduce vertices
- Live preview with committed + cursor segments
- Single undo operation for entire polyline
- Clean implementation with minimal code changes
