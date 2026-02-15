# Debug Mode Decoupling - Metrics Always Visible

## What Changed

### Before
- **Debug button** toggled BOTH:
  1. Visual debug overlays (nearest point, intersections, debug layer)
  2. Fill metrics display (steps, states, ms, candMax, abort)
  3. DEBUG badge visibility

- Metrics were hidden when debug mode was OFF
- DEBUG badge was hidden when debug mode was OFF

### After
- **Debug button** toggles ONLY:
  1. Visual debug overlays (debugLayer visibility)
  2. Debug intersections rendering
  3. Fill trace visualization

- **Fill metrics** are **ALWAYS VISIBLE** (static)
- **DEBUG badge** is **ALWAYS VISIBLE** (static)
- Metrics and badge do NOT toggle on/off with debug mode

## Why This Change

**User Request:** "Do not link debug mode which show the distant of nearest point and crossing with upper line debug information which is need to be static do not turn off an on"

**Translation:**
- Visual debug features (nearest point distance, crossing points) → Controlled by Debug button
- Upper toolbar metrics (fill stats) → Always visible (static)
- These two should NOT be linked

## Implementation

### 1. CSS Changes (web/index.html)

**Removed conditional visibility classes:**
```css
/* BEFORE: */
.debug-badge {
  display: none;  /* Hidden by default */
}
.debug-badge.active {
  display: inline-block;  /* Show when debug on */
}

#fillMetrics {
  display: none;  /* Hidden by default */
}
#fillMetrics.active {
  display: inline-block;  /* Show when debug on */
}

/* AFTER: */
.debug-badge {
  display: inline-block;  /* Always visible */
  background: #ef4444;
  color: white;
  /* ... */
}

#fillMetrics {
  display: inline-block;  /* Always visible */
  font-family: monospace;
  color: #999;
  /* ... */
}
```

### 2. JavaScript Changes (web/main.js)

**Removed metrics toggle from debug handler:**
```javascript
// BEFORE:
case "ToggleDebug": {
  debugMode = debug;
  if (!debugMode) {
    debugLayer.style.display = 'none';
    fillMetrics.classList.remove('active');  // ← Removed
    debugBadge.classList.remove('active');   // ← Removed
  } else {
    debugLayer.style.display = 'block';
    fillMetrics.classList.add('active');     // ← Removed
    debugBadge.classList.add('active');      // ← Removed
  }
}

// AFTER:
case "ToggleDebug": {
  debugMode = debug;
  if (!debugMode) {
    debugLayer.style.display = 'none';
    fillTraceLayer.replaceChildren();
    fillDebugLayer.replaceChildren();
    // Metrics and badge are now STATIC (always visible)
  } else {
    debugLayer.style.display = 'block';
    updateDebugIntersections();
  }
}
```

## Behavior Now

### Debug Button = Visual Overlays Only

**When Debug is ON:**
- ✅ Shows debugLayer (SVG overlay)
- ✅ Shows nearest point/line indicators
- ✅ Shows intersection dots
- ✅ Updates debug intersections
- ✅ Fill trace visualization visible

**When Debug is OFF:**
- ✅ Hides debugLayer
- ✅ Clears fill trace layer
- ✅ Clears fill debug layer
- ✅ No intersection dots visible

### Metrics Bar = Always Visible

**Regardless of Debug state:**
- ✅ DEBUG badge always shows (red badge)
- ✅ Fill metrics always show
- ✅ Updates after each fill operation
- ✅ Shows: `fill: steps=X states=Y ms=Z candMax=N abort=REASON`

## UI Layout

```
┌──────────────────────────────────────────────────────────────┐
│ rustroke  [DEBUG]  Lines: 5  fill: steps=123 states=45...   │  ← ALWAYS visible
│ [Undo] [Clear] [Trim] [Fill] [🎨] [Frame] [Debug] [Graph]   │
└──────────────────────────────────────────────────────────────┘
                            ↓
                  Floating over canvas
                            
Canvas with:
- Lines (always unless hidden with Hide button)
- Fills (always)
- Debug overlays (ONLY when Debug button active)
- Graph visualization (ONLY when Graph button active)
```

## What Each Button Controls

| Button | Controls | Always Visible |
|--------|----------|----------------|
| **Debug** | Visual debug overlays (nearest point, intersections) | No (toggles) |
| **Graph** | Graph visualization (nodes, edges, DSU) | No (toggles) |
| **Metrics** | Fill stats (steps, states, ms, abort) | **Yes (static)** |
| **DEBUG Badge** | Indicator in toolbar | **Yes (static)** |
| **Line Counter** | "Lines: N" display | **Yes (static)** |

## Files Modified

1. **web/index.html**
   - Lines 71-97: Removed `.active` conditional visibility
   - Made `.debug-badge` and `#fillMetrics` always `display: inline-block`

2. **web/main.js**
   - Lines 207-224: Removed metrics/badge toggle from debug handler
   - Added comment explaining static behavior

## Testing

### Test 1: Metrics Always Visible
1. Open http://localhost:8080/
2. ✅ See "DEBUG" badge and metrics immediately (no need to click anything)
3. Draw a closed shape and fill it
4. ✅ Metrics update with actual values
5. Click Debug button on/off multiple times
6. ✅ Metrics stay visible throughout

### Test 2: Debug Visual Overlays
1. Draw several lines
2. Click Debug button → ON
3. ✅ See blue nearest point indicators, intersection dots
4. Move cursor around
5. ✅ Debug overlays update in real-time
6. Click Debug button → OFF
7. ✅ Overlays disappear
8. ✅ Metrics still visible!

### Test 3: Independent Controls
1. Metrics visible (always)
2. Click Debug → Overlays appear
3. Click Graph → Graph visualization appears
4. Click Debug → Overlays hide, graph stays, metrics stay
5. Click Graph → Graph hides, metrics stay
6. ✅ All controls independent

## Build Status

```
✅ Build: Success
✅ WASM: 69.05 KB (unchanged)
✅ JS: 19.22 kB
✅ HTML: 6.31 kB
✅ Zero warnings, zero errors
```

## Summary

**Key Changes:**
- ✅ Debug button controls ONLY visual overlays
- ✅ Metrics are static (always visible)
- ✅ DEBUG badge is static (always visible)
- ✅ No coupling between toolbar info and debug visuals

**User Benefits:**
- 📊 Always see fill statistics without toggling
- 🎯 Debug button has clear single purpose (visual overlays)
- 🚀 Simpler mental model (metrics don't disappear)
- 👁️ Better visibility into app state at all times

The toolbar metrics now serve as a **persistent status bar** showing real-time app statistics, while the Debug button purely controls **temporary visual overlays** for development/debugging.
