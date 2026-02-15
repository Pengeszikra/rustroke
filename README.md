# Rustroke

A minimalist vector drawing app built with Rust/WASM and vanilla JavaScript.

## Features

- **Freehand drawing** – smooth polylines with pointer/touch support
- **Smart fill** – click-to-fill closed shapes with boundary detection
- **Trim** – remove lines inside filled regions
- **Undo** – full undo stack for all operations
- **PNG export** – high-DPI export matching screen view
- **Frame mode** – draw rectangular borders
- **Debug overlays** – visualize graph structure, nearest segments, fill candidates
- **Always-on metrics** – comprehensive debug line for freeze detection
- **Hide lines toggle** – view fills only (lines still exist in data)
- **Minimal dependencies** – Rust + WASM + vanilla JS, no framework bloat

## Why

Most drawing apps rely on heavy frameworks or raster-only rendering. Rustroke proves you can build a fast, responsive vector editor with a tiny footprint: Rust for graph logic, WASM for portability, and plain JS for the UI. The entire app is under 100KB (WASM + JS combined).

The focus is on **stability and diagnosability**: the always-visible debug line tracks every subsystem in real-time, making it possible to diagnose freezes and performance issues even on low-end mobile devices like the Samsung Galaxy Tab S3.

## UI Overview

**Toolbar buttons** (floating at the top):

- **Undo** – revert last action (Ctrl+Z)
- **Clear** – delete all lines and fills
- **Trim** – remove line segments inside filled regions
- **Fill** – click near a closed shape to fill it
- **Frame** – draw a rectangular border
- **Debug** – toggle debug overlays (nodes, tangents, sectors)
- **Graph** – toggle graph structure visualization
- **Hide Lines** – hide/show line strokes (fills remain visible)
- **PNG** – export current view as PNG (respects hide state)
- **Color picker** – choose fill color

**Status area** (below title):

- **Lines:** – count of drawn polylines
- **Debug metrics** – three-line readout with system health (see below)

## Debug Line

The debug line is **always visible** and updates every 250ms. It displays real-time metrics across three lines for comprehensive system monitoring.

Example output: `pts:15 tool:draw ptr:down move/s:120 fps:60 dt:16ms long:0 wasm/s:45 uiAge:0ms evtAge:12ms wasmAge:8ms lastEvt:pmove#1 lastWasm:editor_add_segment lastWasmMs:1.2 maxWasmMs:3.4 spikes:0 mem:4pg/256k heap:12.3M err:-`

**What to look for when hunting freezes:**

- **uiAge exploding** (e.g. `uiAge:5234ms`) → UI thread frozen (JS infinite loop)
- **wasmAge exploding** (e.g. `wasmAge:5234ms`) → WASM call hung (fill algorithm infinite loop)
- **evtAge exploding** (e.g. `evtAge:5234ms`) → pointer events stopped (capture issue)
- **fps dropping to 0** → confirm freeze (cross-check with `dt` and `long` frame count)
- **spikes increasing** → track slow WASM calls (>50ms threshold)
- **lastWasm + lastWasmMs** → identify which operation was running when freeze occurred
- **mem/heap growing unbounded** → memory leak or GC pressure

After a freeze, screenshot the toolbar to see exactly which subsystem stopped updating.

## Tech Stack

- **Rust** – core drawing engine, graph algorithms, fill traversal
- **wasm-bindgen** – Rust ↔ JS interop
- **wasm-pack** – WASM build tooling
- **Vanilla JavaScript** – UI, pointer events, SVG rendering
- **Vite** – dev server and bundler
- **SVG** – canvas rendering (1:1 pixel mapping, no transforms)

## Getting Started

**Prerequisites:** Node.js 18+, Rust toolchain, wasm-pack

**Install dependencies:**

```bash
npm install
```

**Build WASM module:**

```bash
wasm-pack build --target web
```

**Run dev server:**

```bash
npm run dev
```

Open `http://localhost:8080` in your browser. Start drawing!

## Build & Deploy

**Production build:**

```bash
npm run build
```

Output goes to `dist/`. Serve statically or deploy to Vercel/Netlify/GitHub Pages.

**Test production build locally:**

```bash
npm run preview
```

## Mobile Notes (Galaxy Tab S3)

The app targets mobile/tablet browsers but freezes on Samsung Galaxy Tab S3 around 40–60 drawn lines. Current debugging efforts:

- ✅ Always-visible debug line with freeze detection timestamps
- ✅ Event ring buffer (accessible via console: `eventRing.buffer`)
- ✅ WASM call duration tracking with spike detection
- ✅ Watchdog ages: `uiAge`, `evtAge`, `wasmAge`
- ⏳ Root cause analysis in progress (likely fill algorithm edge case)

**Known workarounds:**

- Keep line count below 40 for stable fills
- Use Trim frequently to reduce graph complexity
- Avoid overlapping/collinear segments when possible

## Contributing

This is a minimal research project. PRs welcome for:

- Bug fixes (especially freeze-related)
- Performance improvements
- Mobile compatibility fixes
- Documentation improvements

Please keep changes **minimal and surgical** – avoid refactors or new dependencies unless absolutely necessary.

## License

MIT – see [LICENSE](LICENSE) file for details.
