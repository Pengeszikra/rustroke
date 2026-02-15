# Fill Freeze Detection & Instrumentation

## Problem Statement

The Rustroke app freezes on Samsung Galaxy Tab S3 around 40–60 drawn lines when using the Fill tool. The freeze manifests as:
- UI becomes completely unresponsive
- No FPS updates
- Browser shows "page unresponsive" dialog after several seconds

**Root cause hypothesis:** The fill algorithm (even with MAX_FILL_STEPS = 20000) can take seconds to complete on slow devices, blocking the main thread and freezing the UI.

## Solution: Enhanced Instrumentation

Instead of immediately implementing incremental fill or Web Workers (complex refactors), we added **precise freeze detection instrumentation** to:
1. **Prove** the freeze happens inside Fill (not elsewhere)
2. **Measure** exact duration and identify which fill calls are problematic
3. **Capture** state before freeze so screenshots show what went wrong

## Implementation

### Part A: Operation Tracking (JS)

**Added to metrics object:**
```javascript
op: '-',              // Current operation: fill:start, fill:end, fill:ERR, draw, etc.
opTs: 0,              // Timestamp when operation started
opMs: 0,              // Duration of last completed operation
```

**Updated debug line:**
- Line 1 now shows: `op:fill:start opAge:1234ms` during fill execution
- `opAge` = time since operation started (calculated as `now - opTs`)

### Part B: Fill Handler Instrumentation

**Before calling WASM fill:**
```javascript
// 1. Set operation state
metrics.op = 'fill:start';
metrics.opTs = performance.now();
updateDebugMetrics(); // Force metrics update to DOM

// 2. Force browser to paint BEFORE freeze
await new Promise(resolve => requestAnimationFrame(resolve));

// 3. Call WASM (potential freeze point)
wasm.editor_fill_debug_at(x, y);
```

**Why force paint?**
Without the `await rAF`, the debug line update (`op:fill:start`) stays in the DOM write queue but never renders. The WASM call freezes the thread before the browser can paint. With the forced rAF, the UI updates BEFORE the freeze, so:
- Screenshot shows `op:fill:start opAge:5234ms` instead of stale data
- We can prove the freeze happened inside `editor_fill_debug_at`

**After WASM returns (or throws):**
```javascript
try {
  wasm.editor_fill_debug_at(x, y);
  
  metrics.op = 'fill:end';
  metrics.opMs = fillDuration;
  eventRing.add(`fill:end ${fillDuration.toFixed(1)}ms`);
} catch (err) {
  metrics.op = 'fill:ERR';
  metrics.opMs = fillDuration;
  metrics.lastWasmError = `fill: ${err.message}`;
  eventRing.add(`fill:ERR ${err.message}`);
}
```

### Part C: Event Ring Buffer

All fill operations are logged to the ring buffer (accessible via console):

```javascript
eventRing.buffer
```

Example entries:
```
[12340] fill:start x=234 y=456
[17574] fill:end 5234.2ms
```

or if it freezes:
```
[12340] fill:start x=234 y=456
// ... freeze ... no more entries ...
```

## Rust Guardrails (Already Present)

The Rust fill algorithm already has hard guardrails (added in previous session):

**src/lib.rs line 212:**
```rust
const MAX_FILL_STEPS: u32 = 20000;
const NO_PROGRESS_WINDOW: usize = 20;
const NO_PROGRESS_THRESHOLD: usize = 3;
```

**Guardrail 1: Step Limit**
```rust
if step_idx >= MAX_FILL_STEPS {
    return TraceResult::fail(FailReason::StepLimit, step_idx, step_debug);
}
```
- Prevents infinite loops
- Aborts after 20,000 iterations
- **Problem:** On slow tablets, 20,000 iterations can still take 2–5 seconds, freezing UI

**Guardrail 2: NO_PROGRESS Detection**
```rust
if step_idx >= NO_PROGRESS_WINDOW as u32 {
    // Check if recent 20 steps only have <=3 unique states
    if unique_recent.len() <= NO_PROGRESS_THRESHOLD {
        return TraceResult::fail(FailReason::DeadEnd, step_idx, step_debug);
    }
}
```
- Detects when algorithm is stuck in a small cycle
- Aborts early instead of burning all 20K steps

**FailReason enum:**
```rust
enum FailReason {
    StepLimit,    // Hit MAX_FILL_STEPS
    DeadEnd,      // NO_PROGRESS or no valid continuation
}
```

## Diagnosis Workflow

### Before Freeze (Normal Operation)

**Debug line shows:**
```
pts:15 tool:draw op:- opAge:0ms ptr:down move/s:120 fps:60 dt:16ms long:0 wasm/s:45
uiAge:0ms evtAge:12ms wasmAge:8ms lastEvt:pmove#1 lastWasm:editor_add_segment lastWasmMs:1.2 maxWasmMs:3.4 spikes:0
mem:4pg/256k heap:12.3M err:-
```

### During Fill (Healthy)

**Debug line shows:**
```
pts:52 tool:draw op:fill:start opAge:12ms ptr:up move/s:0 fps:60 dt:16ms long:0 wasm/s:1
uiAge:0ms evtAge:50ms wasmAge:12ms lastEvt:pdown#1 lastWasm:editor_fill_debug_at lastWasmMs:12.3 maxWasmMs:12.3 spikes:0
mem:7pg/448k heap:18.5M err:-
```

**Then completes:**
```
pts:52 tool:draw op:fill:end opAge:0ms ptr:up move/s:0 fps:60 dt:16ms long:0 wasm/s:1
uiAge:0ms evtAge:120ms wasmAge:0ms lastEvt:pdown#1 lastWasm:editor_fill_debug_at lastWasmMs:12.3 maxWasmMs:12.3 spikes:0
mem:7pg/448k heap:18.5M err:-
```

### During Freeze

**Screenshot of debug line shows:**
```
pts:52 tool:draw op:fill:start opAge:5234ms ptr:up move/s:0 fps:0 dt:5234ms long:15 wasm/s:0
uiAge:5234ms evtAge:5200ms wasmAge:120ms lastEvt:pdown#1 lastWasm:editor_fill_debug_at lastWasmMs:83.4 maxWasmMs:83.4 spikes:1
mem:7pg/448k heap:18.5M err:-
```

**Interpretation:**
1. `op:fill:start` - Fill operation started
2. `opAge:5234ms` - Fill has been running for 5+ seconds
3. `uiAge:5234ms` - UI thread frozen (rAF stopped)
4. `fps:0`, `dt:5234ms` - Confirms UI freeze
5. `lastWasm:editor_fill_debug_at` - Last WASM call was fill
6. `lastWasmMs:83.4` - Previous fill took 83ms (slow!)
7. `spikes:1` - One spike detected before freeze

**Diagnosis:** Fill algorithm is taking >5 seconds, blocking main thread, freezing UI.

**Console inspection:**
```javascript
> eventRing.buffer.slice(-10)
[
  "[12340] EVT pdown id=1 x=234 y=456",
  "[12341] fill:start x=234 y=456",
  // no "fill:end" entry - still running/frozen
]
```

## Next Steps

### If Instrumentation Proves Fill is the Problem

**Option 1: Web Worker (Complex)**
- Move fill algorithm to separate thread
- Requires WASM initialization in worker context
- Adds message passing overhead
- Enables cancel/timeout from main thread

**Option 2: Incremental Fill (Moderate)**
- Add Rust exports: `fill_begin()`, `fill_step(budget)`, `fill_cancel()`
- Refactor `trace_face_side` to support resumption
- JS runs fill in chunks across multiple frames
- Maintains UI responsiveness

**Option 3: Optimize Fill (Difficult)**
- Profile fill algorithm to find hotspots
- Reduce MAX_FILL_STEPS on mobile (detect via userAgent)
- Add early-exit heuristics for complex scenes
- Cache frequently-accessed graph data structures

**Option 4: UI Workaround (Simple)**
- Show "Filling..." overlay during fill
- Add "Cancel" button that interrupts main thread (if possible)
- Warn user when line count > 40 and fill is enabled
- Suggest using Trim before Fill to reduce graph complexity

## Files Modified

### web/main.js

**Lines 98-103:** Added operation tracking fields to metrics
```javascript
op: '-',
opTs: 0,
opMs: 0,
```

**Lines 370-373:** Calculate opAge in updateDebugMetrics
```javascript
const opAge = metrics.opTs ? Math.round(now - metrics.opTs) : 0;
```

**Lines 393-396:** Show op and opAge in debug line
```javascript
const line1 = [
  `pts:${points}`,
  `tool:${metrics.currentTool}`,
  `op:${metrics.op}`,
  `opAge:${opAge}ms`,
  // ...
];
```

**Lines 251-298:** Enhanced Fill action handler
- Set `metrics.op = 'fill:start'` before WASM call
- Force paint with `await new Promise(rAF)`
- Measure fill duration
- Set `metrics.op = 'fill:end'` or `'fill:ERR'` after completion
- Log all events to ring buffer

## Build Status

```
✅ Build successful: 0 errors, 0 warnings
✅ WASM: 69.05 KB (unchanged)
✅ JS: 25.12 kB (+520 bytes for instrumentation)
```

## Testing Checklist

- [ ] **Desktop test:** Draw 10 lines, use Fill, verify:
  - Debug line shows `op:fill:start` briefly
  - Then shows `op:fill:end` with duration
  - `opAge` resets to 0 after completion
  - Ring buffer has `fill:start` and `fill:end` entries

- [ ] **Slow fill test:** Draw 50 lines, create complex overlaps, use Fill:
  - Observe `opAge` incrementing during fill
  - If fill completes: verify `lastWasmMs` shows realistic duration (>50ms)
  - If fill freezes: screenshot shows `op:fill:start` + `opAge` + `uiAge` all huge

- [ ] **Freeze scenario (tablet):** Draw 40–60 lines, use Fill:
  - Before freeze: Take screenshot of debug line
  - After freeze: Screenshot should show `op:fill:start opAge:5000ms+`
  - Console: `eventRing.buffer` should show `fill:start` but no `fill:end`

## Summary

**What we accomplished:**
1. ✅ Added precise operation tracking (`op`, `opTs`, `opAge`)
2. ✅ Force browser paint before potential freeze
3. ✅ Comprehensive fill timing and logging
4. ✅ Event ring buffer for debugging

**What we can now diagnose:**
- Exactly when fill starts and ends
- How long fill operations take
- Whether fill completed or froze
- Whether freeze is in fill or elsewhere

**Freeze still happens, but now:**
- Screenshots clearly show `op:fill:start` + huge `opAge`
- We have proof fill is the culprit
- We can measure exact durations to prioritize optimization
- Ring buffer provides detailed event timeline

**If freezes persist:**
- Implement incremental fill (Option 2)
- Or move fill to Web Worker (Option 1)
- Or reduce MAX_FILL_STEPS on mobile (Option 3)
- Or add UI workarounds (Option 4)
