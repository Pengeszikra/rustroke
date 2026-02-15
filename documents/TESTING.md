# Rustroke Testing Guide

## Quick Test

```bash
# Run automated WASM tests (Node.js)
npm test

# Expected output:
# ✅ All tests passed! WASM module is healthy.
# 📊 Results: 23/23 tests passed
```

## Browser Self-Test

```bash
# Start dev server and open self-test page
npm run test:browser

# Or manually:
npm run dev
# Then open: http://localhost:8080/self-test.html
```

The self-test page will automatically run all tests and display results in a visual dashboard.

## What's Tested

### Core Functions (4 tests)
- ✅ WASM exports present
- ✅ Editor initialization
- ✅ Memory buffer accessible
- ✅ Export pointers valid

### Drawing Operations (4 tests)
- ✅ Add single line
- ✅ Add multiple lines
- ✅ Zero-length line rejected
- ✅ Export data updates

### Undo & History (4 tests)
- ✅ Undo single line
- ✅ Undo multiple operations
- ✅ Clear canvas
- ✅ Undo after clear

### Graph & Fill (4 tests)
- ✅ Fill graph builds
- ✅ Closed component detection
- ✅ Debug mode toggle
- ✅ Cleanup overhangs
- ✅ Frame addition

## Manual Testing

After automated tests pass, test these features manually:

### Basic Drawing
1. Draw several lines by clicking and dragging
2. Lines should appear immediately
3. Preview line shows while dragging

### Fill Tool
1. Draw a closed triangle
2. Click "Fill" button (turns blue)
3. Click inside triangle
4. Triangle should fill with color
5. **Note**: If no fill appears, check:
   - Is the shape actually closed?
   - Try clicking "Graph" to see closed components
   - Open/dangling lines won't fill (this is correct behavior)

### Debug Mode
1. Click "Debug" button
2. Should show intersection points as circles
3. Click again to hide

### Graph Mode
1. Click "Graph" button  
2. Should show cut segments and nodes
3. Closed components shown in one color
4. Open segments may be filtered out

### Undo/Redo
1. Draw 3 lines
2. Click "Undo" → Last line disappears
3. Click "Undo" again → Second line disappears
4. Works for fills too

### Trim Overhangs
1. Draw a closed shape (triangle)
2. Draw a separate dangling line
3. Click "Trim"
4. Dangling line should disappear
5. Closed shape remains

### Frame
1. Click "Frame"
2. 4 lines appear at canvas edges
3. Can be filled (creates boundary)

### Hide/Show Lines
1. Click "Hide" button
2. Lines disappear (fills remain)
3. Click "Show" to restore

## Known Behaviors (Not Bugs)

### Fill Doesn't Work Sometimes
- **Cause**: Closed component filtering
- **Expected**: Only closed shapes can be filled
- **Fix**: Draw complete closed shapes (triangle, rectangle, etc.)
- **Test**: Draw a single line → Try fill → No fill created (correct!)

### Fill Click Outside Shape
- **Cause**: No closed component near click
- **Expected**: Fill mode auto-disables
- **Console**: `[Fill] No area found, fill mode disabled`

### Trim Overhangs Removes Everything
- **Cause**: No closed components in drawing
- **Expected**: All lines are dangling, so all removed
- **Test**: Draw closed triangle first → Then dangling line → Trim → Only dangling removed

### Graph Button Shows Nothing
- **Cause**: No closed components
- **Expected**: Graph debug only shows closed component structure
- **Fix**: Draw closed shapes

## Troubleshooting

### Tests Fail
```bash
# Rebuild WASM
cargo +nightly build --release --target wasm32-unknown-unknown

# Copy to web
cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/wasm/rustroke.wasm

# Retest
npm test
```

### Browser Console Errors
1. Open DevTools (F12)
2. Check Console tab for errors
3. Common issues:
   - `editor_* is not a function` → WASM not loaded, hard refresh
   - `undefined is not a function` → Clear cache, rebuild
   - No errors but nothing works → Check Network tab for WASM 404

### Self-Test Page Shows Failures
1. Check which tests fail
2. Look at console output for details
3. Most common: Fill/Graph tests fail if shapes aren't closed

### Performance Issues
1. Drawing slow → Too many lines (>1000)
2. Fill slow → Complex intersections
3. Trim hangs → Very large graph (>5000 segments)

## Test Coverage

| Feature | Node Test | Browser Test | Manual Test |
|---------|-----------|--------------|-------------|
| Line drawing | ✅ | ✅ | ✅ |
| Undo/Redo | ✅ | ✅ | ✅ |
| Clear | ✅ | ✅ | ✅ |
| Fill | ⚠️ Basic | ✅ | ✅ Required |
| Debug mode | ⚠️ Toggle | ✅ | ✅ Required |
| Graph mode | ⚠️ Toggle | ✅ | ✅ Required |
| Trim overhangs | ✅ | ✅ | ✅ |
| Frame | ✅ | ✅ | ✅ |
| Hide/Show | ❌ | ❌ | ✅ Required |
| Recording | ❌ | ❌ | ✅ Required |

## Stress Tests

### Rapid Drawing
- Hold mouse button, scribble rapidly for 10 seconds
- Should not crash or lag severely
- Lines should all appear

### Rapid Undo
- Add 50+ lines
- Spam undo button rapidly
- Should not crash
- All lines should be removed in order

### Fill Spam
- Draw closed triangle
- Click fill 20 times rapidly inside
- Should create multiple overlapping fills
- Should not crash

### Mixed Chaos
- Randomly: draw, fill, undo, trim, clear
- Continue for 30 seconds
- Should remain stable

## Success Criteria

✅ **All Tests Pass**
- Node tests: 23/23
- Browser self-test: All green
- No console errors

✅ **Core Features Work**
- Can draw lines
- Can fill closed shapes
- Undo works
- Clear works

✅ **No Crashes**
- Stress tests complete without errors
- No infinite loops
- No memory issues

## Getting Help

If tests fail after following troubleshooting:

1. Check `INTEGRITY_SAFETY.md` for integrity check details
2. Run debug build: `cargo +nightly build --target wasm32-unknown-unknown`
3. Look for assertion failures in debug mode
4. Open issue with:
   - Test output
   - Browser console log
   - Steps to reproduce
