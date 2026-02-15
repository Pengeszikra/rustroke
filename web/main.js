/**
 * Rustroke - Main Application Entry Point
 */

import initWasm from './wasm/rustroke.js';

// Disable console logging (keep errors)
const originalLog = console.log;
const originalWarn = console.warn;
console.log = () => {};
console.warn = () => {};

// Wait for DOM and initialize
async function main() {
  console.log('[Rustroke] Initializing...');

    const canvas = document.getElementById('canvas');
    const linesGroup = document.getElementById('lines');
    const fillsGroup = document.getElementById('fills');
    const preview = document.getElementById('preview');
    const undoBtn = document.getElementById('undoBtn');
    const clearBtn = document.getElementById('clearBtn');
    const cleanBtn = document.getElementById('cleanBtn');
    const fillBtn = document.getElementById('fillBtn');
    const fillColor = document.getElementById('fillColor');
    const addFrameBtn = document.getElementById('addFrameBtn');
    const exportPngBtn = document.getElementById('exportPngBtn');
    const debugBtn = document.getElementById('debugBtn');
    const graphDebugBtn = document.getElementById('graphDebugBtn');
    const toggleLinesBtn = document.getElementById('toggleLinesBtn');
    const debugBadge = document.getElementById('debugBadge');
    const lineCounter = document.getElementById('lineCounter');
    const debugLayer = document.getElementById('debugLayer');
    const debugNearestLine = document.getElementById('debugNearestLine');
    const debugNearestPoint = document.getElementById('debugNearestPoint');
    const debugRay = document.getElementById('debugRay');
    const debugIntersectionsGroup = document.getElementById('debugIntersections');
    const fillTraceLayer = document.getElementById('fillTraceLayer');
    const graphDebugLayer = document.getElementById('graphDebugLayer');
    const fillDebugLayer = document.getElementById('fillDebugLayer');
    const viewBox = canvas.viewBox.baseVal;

    let wasm = null;
    let dragging = false;
    let fillMode = false;
    let debugMode = false;
    let graphDebugMode = false;
    let showLines = true;
    let isFilling = false; // GUARDRAIL: Re-entrancy lock
    
    // Recording/Playback state
    let isRecording = false;
    let isPlaying = false;
    let recordStart = 0;
    let events = [];
    let recording = { version: 1, createdAt: 0, events: [] };
    
    /** @type {{x: number, y: number}} */
    let startPoint = { x: 0, y: 0 };

    // ==== DEBUG METRICS COLLECTOR ====
    const metrics = {
      // Pointer events
      pointerMoveCount: 0,
      pointerDownCount: 0,
      pointerUpCount: 0,
      pointerCancelCount: 0,
      lastPointerId: -1,
      lastX: 0,
      lastY: 0,
      lastPressure: -1,
      lastPointerType: '-',
      pointerState: 'up',
      lastEvtType: '-',
      
      // Frame timing
      lastFrameTime: 0,
      frameDt: 0,
      frameDtSum: 0,
      frameCount: 0,
      fps: 0,
      longFrameCount: 0,
      
      // WASM call tracking
      wasmCallCount: 0,
      wasmCallsPerSec: 0,
      lastWasmCall: '-',
      lastWasmDuration: 0,
      maxWasmDuration: 0,
      lastWasmError: 'NONE',
      wasmSpikeCount: 0,
      
      // Watchdog timestamps
      lastRafTs: 0,
      lastEvtTs: 0,
      lastWasmOkTs: 0,
      
      // Tool state
      currentTool: 'draw',
      
      // Error tracking
      lastJsError: 'NONE',
      
      // Reset counters (for per-second rates)
      reset() {
        const now = performance.now();
        if (!this.lastResetTime) this.lastResetTime = now;
        const dt = now - this.lastResetTime;
        if (dt >= 1000) {
          this.pointerMoveRate = Math.round((this.pointerMoveCount / dt) * 1000);
          this.wasmCallsPerSec = Math.round((this.wasmCallCount / dt) * 1000);
          this.pointerMoveCount = 0;
          this.wasmCallCount = 0;
          this.lastResetTime = now;
        }
      },
      
      pointerMoveRate: 0,
      lastResetTime: 0
    };

    // Event ring buffer for debugging (last 40 events)
    const eventRing = {
      buffer: [],
      maxSize: 40,
      add(msg) {
        const entry = `[${Math.round(performance.now())}] ${msg}`;
        this.buffer.push(entry);
        if (this.buffer.length > this.maxSize) {
          this.buffer.shift();
        }
      },
      getLast(n) {
        return this.buffer.slice(-n);
      }
    };

    // Wrap WASM calls for instrumentation
    let wasmRaw = null; // Store raw WASM module
    const wasmWrapper = {
      _wrapCall(name, fn) {
        return (...args) => {
          const start = performance.now();
          metrics.wasmCallCount++;
          metrics.lastWasmCall = name;
          try {
            const result = fn(...args);
            const duration = performance.now() - start;
            metrics.lastWasmDuration = duration;
            
            if (duration > metrics.maxWasmDuration) {
              metrics.maxWasmDuration = duration;
            }
            
            // Track spikes (>50ms)
            if (duration > 50) {
              metrics.wasmSpikeCount++;
              eventRing.add(`WARN wasm spike ${name} ${duration.toFixed(1)}ms`);
            }
            
            metrics.lastWasmOkTs = performance.now();
            eventRing.add(`WASM ${name} ok ${duration.toFixed(1)}ms`);
            return result;
          } catch (error) {
            const duration = performance.now() - start;
            metrics.lastWasmError = `${name}: ${error.message?.slice(0, 30) || 'unknown'}`;
            eventRing.add(`ERR WASM ${name} failed: ${error.message?.slice(0, 40) || 'unknown'}`);
            throw error;
          }
        };
      }
    };

    // Global error handlers
    window.addEventListener('error', (evt) => {
      metrics.lastJsError = evt.message?.slice(0, 50) || 'unknown';
      eventRing.add(`ERR JS: ${evt.message?.slice(0, 60) || 'unknown'}`);
    });
    
    window.addEventListener('unhandledrejection', (evt) => {
      metrics.lastJsError = `Promise: ${evt.reason?.message?.slice(0, 40) || 'unknown'}`;
      eventRing.add(`ERR Promise: ${evt.reason?.message?.slice(0, 50) || 'unknown'}`);
    });

    /**
     * Central action dispatcher
     * Routes all actions through one point for recording/playback
     * @param {Object} action - {type: string, data: Object}
     * @param {Object} options - {source: "user"|"playback"}
     */
    function dispatch(action, options = {source: "user"}) {
      dispatchAsync(action, options); // Fire and forget for sync calls
    }

    /**
     * Async version of dispatch that returns Promise
     * Used for playback to wait for animations
     */
    async function dispatchAsync(action, options = {source: "user"}) {
      if (!wasm) return;
      
      const {source} = options;
      
      // Record user actions (not playback echoes)
      if (source === "user" && isRecording) {
        recordEvent(action);
      }
      
      // Execute action
      switch (action.type) {
        case "AddLine": {
          const {x1, y1, x2, y2} = action.data;
          
          // If playback, animate the line stroke
          if (source === "playback") {
            await new Promise(resolve => {
              animateLineStroke(x1, y1, x2, y2, () => {
                wasm.editor_add_line(x1, y1, x2, y2);
                renderFromWasm();
                resolve();
              });
            });
          } else {
            // User action: add immediately
            wasm.editor_add_line(x1, y1, x2, y2);
            renderFromWasm();
          }
          break;
        }
        case "AddFrame": {
          const {corners} = action.data;
          // corners = [{x, y}, {x, y}, {x, y}, {x, y}] (4 corners in clockwise order)
          // Use the WASM add_frame function which handles grouped undo
          wasm.editor_add_frame(
            corners[0].x, corners[0].y,
            corners[1].x, corners[1].y,
            corners[2].x, corners[2].y,
            corners[3].x, corners[3].y
          );
          renderFromWasm();
          break;
        }
        case "Fill": {
          const {x, y, color} = action.data;
          const colorBytes = new TextEncoder().encode(color);
          const colorPtr = wasm.memory.buffer.byteLength - 256;
          const colorView = new Uint8Array(wasm.memory.buffer, colorPtr, colorBytes.length);
          colorView.set(colorBytes);
          wasm.editor_set_fill_color(colorPtr, colorBytes.length);
          
          // Check fills count before
          const fillsBefore = wasm.editor_fills_count();
          
          // Use fill_debug_at which uses the fill graph system and creates the fill
          wasm.editor_fill_debug_at(x, y);
          
          // Check if fill was created
          const fillsAfter = wasm.editor_fills_count();
          const fillCreated = fillsAfter > fillsBefore;
          
          // If no fill was created, turn off fill mode
          if (!fillCreated && fillMode) {
            fillMode = false;
            fillBtn.classList.remove('active');
            canvas.style.cursor = 'default';
            console.log('[Fill] No area found, fill mode disabled');
          }
          
          renderFromWasm();
          
          // Update debug visuals if debug mode is on
          if (debugMode) {
            renderFillTrace();
            renderFillDebug();
          }
          break;
        }
        case "Undo": {
          wasm.editor_undo();
          renderFromWasm();
          break;
        }
        case "Clear": {
          wasm.editor_clear();
          renderFromWasm();
          renderFillTrace();
          break;
        }
        case "Clean": {
          wasm.editor_cleanup_overhangs();
          
          // Read debug buffer (contains: total, deleted, kept, nodes)
          const debugLen = wasm.editor_debug_len_f32();
          if (debugLen >= 4) {
            const debugPtr = wasm.editor_debug_ptr_f32();
            const debugArr = new Float32Array(wasm.memory.buffer, debugPtr, 4);
            const total = Math.round(debugArr[0]);
            const deleted = Math.round(debugArr[1]);
            const kept = Math.round(debugArr[2]);
            const nodes = Math.round(debugArr[3]);
            console.log(`[Overhang] segmentsBefore=${total} deleted=${deleted} kept=${kept} nodes=${nodes}`);
          }
          
          renderFromWasm();
          break;
        }
        case "SetFillColor": {
          const {color} = action.data;
          fillColor.value = color;
          const colorBytes = new TextEncoder().encode(color);
          const colorPtr = wasm.memory.buffer.byteLength - 256;
          const colorView = new Uint8Array(wasm.memory.buffer, colorPtr, colorBytes.length);
          colorView.set(colorBytes);
          wasm.editor_set_fill_color(colorPtr, colorBytes.length);
          break;
        }
        case "ToggleShowLines": {
          const {show} = action.data;
          showLines = show;
          toggleLinesBtn.textContent = showLines ? 'Hide' : 'Show';
          console.log('[UI] showLines =', showLines);
          renderFromWasm();
          break;
        }
        case "ToggleDebug": {
          const {debug} = action.data;
          debugMode = debug;
          debugBtn.classList.toggle('active', debug);
          wasm.editor_set_debug(debugMode ? 1 : 0);
          if (!debugMode) {
            debugLayer.style.display = 'none';
            fillTraceLayer.replaceChildren();
            fillDebugLayer.replaceChildren();
            // Metrics and badge are now STATIC (always visible)
          } else {
            debugLayer.style.display = 'block';
            updateDebugIntersections();
          }
          break;
        }
      }
    }

    /**
     * Update debug metrics display (always-on instrumentation with freeze detection)
     */
    function updateDebugMetrics() {
      const debugMetricsEl = document.getElementById('debugMetrics');
      if (!debugMetricsEl) return;
      
      // Get WASM memory info
      let memPages = 'n/a';
      let memBytes = 'n/a';
      if (wasm && wasm.memory) {
        const pages = wasm.memory.buffer.byteLength / 65536;
        memPages = Math.round(pages);
        memBytes = (wasm.memory.buffer.byteLength / 1024).toFixed(0) + 'k';
      }
      
      // Get JS heap if available
      let jsHeap = 'n/a';
      if (performance.memory && performance.memory.usedJSHeapSize) {
        jsHeap = (performance.memory.usedJSHeapSize / 1024 / 1024).toFixed(1) + 'M';
      }
      
      // Calculate watchdog ages (FREEZE DETECTION)
      const now = performance.now();
      const uiAge = metrics.lastRafTs ? Math.round(now - metrics.lastRafTs) : 0;
      const evtAge = metrics.lastEvtTs ? Math.round(now - metrics.lastEvtTs) : 0;
      const wasmAge = metrics.lastWasmOkTs ? Math.round(now - metrics.lastWasmOkTs) : 0;
      
      // Get point/segment count
      let points = 0;
      if (wasm && typeof wasm.editor_line_count === 'function') {
        points = wasm.editor_line_count();
      }
      
      // Format last event type with pointer ID
      const lastEvt = metrics.lastEvtType !== '-' 
        ? `${metrics.lastEvtType}#${metrics.lastPointerId}` 
        : '-';
      
      // Build multiline metrics string with freeze detection fields
      const line1 = [
        `pts:${points}`,
        `tool:${metrics.currentTool}`,
        `ptr:${metrics.pointerState}`,
        `move/s:${metrics.pointerMoveRate}`,
        `fps:${metrics.fps}`,
        `dt:${metrics.frameDt}ms`,
        `long:${metrics.longFrameCount}`,
        `wasm/s:${metrics.wasmCallsPerSec}`
      ].join(' ');
      
      const line2 = [
        `uiAge:${uiAge}ms`,
        `evtAge:${evtAge}ms`,
        `wasmAge:${wasmAge}ms`,
        `lastEvt:${lastEvt}`,
        `lastWasm:${metrics.lastWasmCall}`,
        `lastWasmMs:${metrics.lastWasmDuration.toFixed(1)}`,
        `maxWasmMs:${metrics.maxWasmDuration.toFixed(1)}`,
        `spikes:${metrics.wasmSpikeCount}`
      ].join(' ');
      
      const line3 = [
        `mem:${memPages}pg/${memBytes}`,
        `heap:${jsHeap}`,
        `err:${metrics.lastWasmError !== 'NONE' ? metrics.lastWasmError : (metrics.lastJsError !== 'NONE' ? metrics.lastJsError : '-')}`
      ].join(' ');
      
      debugMetricsEl.textContent = `${line1}\n${line2}\n${line3}`;
    }

    /**
     * Export current canvas view as PNG
     * Clones SVG, removes debug layers, renders to canvas, downloads as PNG
     */
    async function exportAsPNG() {
      try {
        // Clone the SVG element
        const svgClone = canvas.cloneNode(true);
        
        // Remove debug and UI layers from clone
        const debugLayerClone = svgClone.querySelector('#debugLayer');
        const graphDebugLayerClone = svgClone.querySelector('#graphDebugLayer');
        const previewClone = svgClone.querySelector('#preview');
        const fillTraceLayerClone = svgClone.querySelector('#fillTraceLayer');
        const fillDebugLayerClone = svgClone.querySelector('#fillDebugLayer');
        
        if (debugLayerClone) debugLayerClone.remove();
        if (graphDebugLayerClone) graphDebugLayerClone.remove();
        if (previewClone) previewClone.remove();
        if (fillTraceLayerClone) fillTraceLayerClone.remove();
        if (fillDebugLayerClone) fillDebugLayerClone.remove();
        
        // Remove lines if they are hidden
        if (!showLines) {
          const linesClone = svgClone.querySelector('#lines');
          if (linesClone) linesClone.remove();
        }
        
        // Add inline styles to the clone so they are preserved in serialization
        // Lines need stroke styles
        const linesClone = svgClone.querySelector('#lines');
        if (linesClone) {
          const lineElements = linesClone.querySelectorAll('line');
          lineElements.forEach(line => {
            line.setAttribute('stroke', '#000000');
            line.setAttribute('stroke-width', '1');
            line.setAttribute('stroke-linecap', 'round');
          });
        }
        
        // Fills need opacity
        const fillsClone = svgClone.querySelector('#fills');
        if (fillsClone) {
          const polygonElements = fillsClone.querySelectorAll('polygon');
          polygonElements.forEach(polygon => {
            polygon.setAttribute('stroke', 'none');
            if (!polygon.hasAttribute('opacity')) {
              polygon.setAttribute('opacity', '0.7');
            }
          });
        }
        
        // Get current viewBox dimensions
        const vb = canvas.viewBox.baseVal;
        const width = vb.width;
        const height = vb.height;
        
        // Use device pixel ratio for high-resolution export
        const dpr = window.devicePixelRatio || 1;
        const exportWidth = width * dpr;
        const exportHeight = height * dpr;
        
        // Serialize SVG to string
        const svgData = new XMLSerializer().serializeToString(svgClone);
        const svgBlob = new Blob([svgData], { type: 'image/svg+xml;charset=utf-8' });
        const svgUrl = URL.createObjectURL(svgBlob);
        
        // Create offscreen canvas
        const exportCanvas = document.createElement('canvas');
        exportCanvas.width = exportWidth;
        exportCanvas.height = exportHeight;
        const ctx = exportCanvas.getContext('2d');
        
        // Scale for device pixel ratio
        ctx.scale(dpr, dpr);
        
        // Draw canvas background
        ctx.fillStyle = getComputedStyle(canvas).backgroundColor || '#ABABAB';
        ctx.fillRect(0, 0, width, height);
        
        // Load and draw SVG
        const img = new Image();
        img.onload = () => {
          ctx.drawImage(img, 0, 0, width, height);
          URL.revokeObjectURL(svgUrl);
          
          // Convert to PNG blob
          exportCanvas.toBlob((blob) => {
            if (!blob) {
              console.error('[Export] Failed to create PNG blob');
              return;
            }
            
            // Generate filename with timestamp
            const now = new Date();
            const timestamp = now.getFullYear() +
              String(now.getMonth() + 1).padStart(2, '0') +
              String(now.getDate()).padStart(2, '0') + '-' +
              String(now.getHours()).padStart(2, '0') +
              String(now.getMinutes()).padStart(2, '0') +
              String(now.getSeconds()).padStart(2, '0');
            const filename = `rustroke-${timestamp}.png`;
            
            // Trigger download
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = filename;
            a.click();
            URL.revokeObjectURL(url);
            
            console.log(`[Export] PNG saved: ${filename}`);
          }, 'image/png');
        };
        
        img.onerror = () => {
          URL.revokeObjectURL(svgUrl);
          console.error('[Export] Failed to load SVG image');
        };
        
        img.src = svgUrl;
        
      } catch (error) {
        console.error('[Export] PNG export failed:', error);
      }
    }

    /**
     * Animate line stroke during playback
     * Shows preview line animating from start to end
     */
    function animateLineStroke(x1, y1, x2, y2, onComplete) {
      const duration = 200 / 7; // ms to animate stroke (7x faster)
      const startTime = performance.now();
      
      // Show preview line
      preview.classList.add('active');
      preview.setAttribute('x1', x1);
      preview.setAttribute('y1', y1);
      preview.setAttribute('x2', x1);
      preview.setAttribute('y2', y1);
      
      function animate() {
        const elapsed = performance.now() - startTime;
        const progress = Math.min(elapsed / duration, 1);
        
        // Interpolate endpoint
        const currentX = x1 + (x2 - x1) * progress;
        const currentY = y1 + (y2 - y1) * progress;
        
        preview.setAttribute('x2', currentX);
        preview.setAttribute('y2', currentY);
        
        if (progress < 1) {
          requestAnimationFrame(animate);
        } else {
          // Animation complete
          preview.classList.remove('active');
          onComplete();
        }
      }
      
      requestAnimationFrame(animate);
    }

    /**
     * Record an event with timestamp
     */
    function recordEvent(action) {
      const t = performance.now() - recordStart;
      events.push({t, type: action.type, data: action.data});
      console.log(`[Record] ${action.type} at ${t.toFixed(0)}ms`, action.data);
    }

    /**
     * Update UI button states based on recording/playback state
     */
    function updateRecordingUI() {
      recordBtn.disabled = isRecording || isPlaying;
      stopBtn.disabled = !isRecording && !isPlaying;
      playBtn.disabled = isRecording || isPlaying || events.length === 0;
      clearRecordBtn.disabled = isRecording || isPlaying || events.length === 0;
      exportBtn.disabled = isRecording || isPlaying || events.length === 0;
      
      if (isRecording) {
        recordBtn.textContent = 'Recording...';
        recordBtn.classList.add('active');
      } else {
        recordBtn.textContent = 'Record';
        recordBtn.classList.remove('active');
      }
      
      if (isPlaying) {
        playBtn.textContent = 'Playing...';
        playBtn.classList.add('active');
      } else {
        playBtn.textContent = 'Play';
        playBtn.classList.remove('active');
      }
    }

    /**
     * Disable/enable canvas input during playback
     */
    function setCanvasInputEnabled(enabled) {
      if (enabled) {
        canvas.style.pointerEvents = '';
        canvas.style.opacity = '1';
      } else {
        canvas.style.pointerEvents = 'none';
        canvas.style.opacity = '0.7';
      }
    }

    /**
     * Start recording
     */
    function startRecording() {
      isRecording = true;
      recordStart = performance.now();
      events = [];
      recording.createdAt = Date.now();
      console.log('[Recording] Started');
      updateRecordingUI();
    }

    /**
     * Stop recording
     */
    function stopRecording() {
      isRecording = false;
      recording.events = events;
      console.log(`[Recording] Stopped - ${events.length} events`);
      updateRecordingUI();
    }

    /**
     * Play recording
     */
    async function playRecording() {
      if (events.length === 0) return;
      
      isPlaying = true;
      setCanvasInputEnabled(false);
      updateRecordingUI();
      
      // Clear scene before playback
      dispatch({type: "Clear", data: {}}, {source: "playback"});
      
      console.log(`[Playback] Starting - ${events.length} events at 7x speed`);
      
      const startTime = performance.now();
      const speedMultiplier = 7; // 7x speed
      
      // Play events sequentially
      for (let i = 0; i < events.length; i++) {
        if (!isPlaying) break; // Check if stopped
        
        const event = events[i];
        const targetTime = event.t / speedMultiplier; // Divide by 7 for 7x speed
        const currentTime = performance.now() - startTime;
        const delay = Math.max(0, targetTime - currentTime);
        
        // Wait until it's time for this event
        if (delay > 0) {
          await new Promise(resolve => setTimeout(resolve, delay));
        }
        
        if (!isPlaying) break; // Check again after delay
        
        console.log(`[Playback] ${event.type} at ${event.t.toFixed(0)}ms`, event.data);
        
        // Dispatch and wait for completion (especially for AddLine animation)
        await dispatchAsync({type: event.type, data: event.data}, {source: "playback"});
      }
      
      if (isPlaying) {
        finishPlayback();
      }
    }

    /**
     * Stop playback
     */
    function stopPlayback() {
      isPlaying = false;
      preview.classList.remove('active'); // Stop any ongoing animation
      setCanvasInputEnabled(true);
      console.log('[Playback] Stopped');
      updateRecordingUI();
    }

    /**
     * Finish playback naturally
     */
    function finishPlayback() {
      setTimeout(() => {
        isPlaying = false;
        setCanvasInputEnabled(true);
        console.log('[Playback] Finished');
        updateRecordingUI();
      }, 100);
    }

    /**
     * Clear recording
     */
    function clearRecording() {
      events = [];
      recording.events = [];
      console.log('[Recording] Cleared');
      updateRecordingUI();
    }

    /**
     * Export recording to clipboard
     */
    function exportRecording() {
      recording.events = events;
      const json = JSON.stringify(recording, null, 2);
      navigator.clipboard.writeText(json).then(() => {
        console.log('[Export] Recording copied to clipboard');
        alert('Recording copied to clipboard!');
      }).catch(err => {
        console.error('[Export] Failed:', err);
        alert('Export failed. See console for JSON.');
        console.log(json);
      });
    }

    /**
     * Import recording from JSON
     */
    function importRecording() {
      const json = prompt('Paste recording JSON:');
      if (!json) return;
      
      try {
        const imported = JSON.parse(json);
        if (!imported.events || !Array.isArray(imported.events)) {
          throw new Error('Invalid recording format');
        }
        recording = imported;
        events = imported.events;
        console.log(`[Import] Loaded ${events.length} events`);
        alert(`Imported ${events.length} events!`);
        updateRecordingUI();
      } catch (err) {
        console.error('[Import] Failed:', err);
        alert('Import failed: ' + err.message);
      }
    }

    /**
     * Convert pointer event coordinates to SVG viewBox coordinates
     * @param {PointerEvent} evt - pointer event
     * @returns {{x: number, y: number}} point in SVG coordinates
     */
    function toSvgPoint(evt) {
      const rect = canvas.getBoundingClientRect();
      const x = ((evt.clientX - rect.left) / rect.width) * viewBox.width + viewBox.x;
      const y = ((evt.clientY - rect.top) / rect.height) * viewBox.height + viewBox.y;
      return { x, y };
    }

    /**
     * Render lines and fills from WASM memory
     */
    function renderFromWasm() {
      if (!wasm) return;
      const len = wasm.editor_export_len_f32();
      const ptr = wasm.editor_export_ptr_f32();
      
      // Only render lines if showLines is true
      if (showLines) {
        if (!ptr || len === 0) {
          linesGroup.replaceChildren();
          lineCounter.textContent = 'Lines: 0';
        } else {
          const arr = new Float32Array(wasm.memory.buffer, ptr, len);
          const fragments = [];
          for (let i = 0; i < arr.length; i += 4) {
            const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
            line.setAttribute('x1', arr[i + 0]);
            line.setAttribute('y1', arr[i + 1]);
            line.setAttribute('x2', arr[i + 2]);
            line.setAttribute('y2', arr[i + 3]);
            fragments.push(line);
          }
          linesGroup.replaceChildren(...fragments);
          lineCounter.textContent = `Lines: ${wasm.editor_line_count()}`;
        }
      } else {
        // Hide lines but still update status
        linesGroup.replaceChildren();
        if (ptr && len > 0) {
          lineCounter.textContent = `Lines: ${wasm.editor_line_count()} (hidden)`;
        } else {
          lineCounter.textContent = 'Lines: 0';
        }
      }

      // Render fills
      const fillsLen = wasm.editor_export_fills_len();
      const fillsPtr = wasm.editor_export_fills_ptr();
      if (fillsPtr && fillsLen > 0) {
        const fillsArr = new Float32Array(wasm.memory.buffer, fillsPtr, fillsLen);
        const fillFragments = [];
        let i = 0;
        while (i < fillsArr.length) {
          const pointCount = fillsArr[i];
          const r = Math.round(fillsArr[i + 1] * 255);
          const g = Math.round(fillsArr[i + 2] * 255);
          const b = Math.round(fillsArr[i + 3] * 255);
          const a = fillsArr[i + 4];
          i += 5;

          const polygon = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
          let points = '';
          for (let j = 0; j < pointCount && i < fillsArr.length; j++) {
            const x = fillsArr[i];
            const y = fillsArr[i + 1];
            points += `${x},${y} `;
            i += 2;
          }
          polygon.setAttribute('points', points);
          polygon.setAttribute('fill', `rgba(${r},${g},${b},${a})`);
          fillFragments.push(polygon);
        }
        fillsGroup.replaceChildren(...fillFragments);
      } else {
        fillsGroup.replaceChildren();
      }

      // Update intersection dots if debug is on
      if (debugMode) {
        updateDebugIntersections();
      }

      if (graphDebugMode) {
        renderGraphDebug();
      } else {
        graphDebugLayer.replaceChildren();
      }
    }

    /**
     * Update debug intersection dots from cached intersections in Rust
     */
    function updateDebugIntersections() {
      const intLen = wasm.editor_intersections_len_f32();
      const intPtr = wasm.editor_intersections_ptr_f32();
      debugIntersectionsGroup.replaceChildren();

      if (intLen <= 0) return;

      const intArr = new Float32Array(wasm.memory.buffer, intPtr, intLen);
      const count = intArr[0];

      for (let i = 0; i < count; i++) {
        const x = intArr[1 + i * 2];
        const y = intArr[1 + i * 2 + 1];
        const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
        circle.setAttribute('cx', x);
        circle.setAttribute('cy', y);
        circle.setAttribute('r', 3);
        circle.setAttribute('fill', '#ef4444');
        debugIntersectionsGroup.appendChild(circle);
      }
    }

    /**
     * Render fill debug diagnostics (both candidate polygons)
     */
    function renderFillDebug() {
      // Only render fill debug visuals in debug mode
      if (!debugMode) {
        fillDebugLayer.replaceChildren();
        return;
      }
      
      fillDebugLayer.replaceChildren();
      
      if (!wasm) return;
      
      // Get fill walk debug data
      const len = wasm.editor_fill_walk_debug_len_f32();
      const ptr = wasm.editor_fill_walk_debug_ptr_f32();
      
      if (len < 12 || !ptr) {
        return;
      }
      
      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      
      // Parse diagnostic data for up to 2 candidates (12 floats each)
      const candidates = [];
      let idx = 0;
      while (idx + 12 <= len) {
        const direction = Math.floor(arr[idx]);
        const pointCount = Math.floor(arr[idx + 1]);
        const signedArea = arr[idx + 2];
        const area = arr[idx + 3];
        const inside = arr[idx + 4] > 0.5;
        const distSq = arr[idx + 5];
        const dist = arr[idx + 6];
        const isSimple = arr[idx + 7] > 0.5;
        const minx = arr[idx + 8];
        const miny = arr[idx + 9];
        const maxx = arr[idx + 10];
        const maxy = arr[idx + 11];
        
        candidates.push({direction, pointCount, area, inside, dist, isSimple});
        idx += 12;
      }
      
      // Log to console
      if (candidates.length > 0) {
        console.log('[Fill Debug] Candidate AB:', candidates[0]);
        if (candidates.length > 1) {
          console.log('[Fill Debug] Candidate BA:', candidates[1]);
        }
      }
    }

    /**
     * Render fill trace from WASM buffer
     */
     function renderFillTrace() {
      // Only render fill trace visuals in debug mode
      if (!debugMode) {
        fillTraceLayer.replaceChildren();
        return;
      }
      
      fillTraceLayer.replaceChildren();

      if (!wasm) return;

      const len = wasm.editor_fill_trace_len_f32();
      const ptr = wasm.editor_fill_trace_ptr_f32();

      if (len <= 1 || !ptr) {
        console.log('[Fill Trace] No trace data');
        return;
      }

      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      const pointCount = Math.floor(arr[0]);

      if (pointCount <= 0) {
        console.log('[Fill Trace] No trace points');
        return;
      }

      const stepTypeNames = ['ORIGIN', 'NEAREST', 'ENDPOINT', 'CHAIN', '?', '?', '?', '?', '?', 'CLOSED'];
      const stepTypeColors = {
        0: '#ff8c00',  // orange
        1: '#00bfff',  // cyan
        2: '#ff1493',  // magenta
        3: '#00ff00',  // green (default for chain)
        9: '#ff0000'   // red
      };

      console.log(`%c[Fill Trace] Found ${pointCount} steps`, 'color: #2563eb; font-weight: bold; font-size: 12px');

      let idx = 1;
      for (let i = 0; i < pointCount && idx < arr.length; i++) {
        const x = arr[idx];
        const y = arr[idx + 1];
        const stepType = Math.floor(arr[idx + 2]);
        idx += 3;

        const stepName = stepTypeNames[stepType] || 'UNKNOWN';
        const stepColor = stepTypeColors[stepType] || '#999999';

        // Log to console
        let logMessage = `  Step ${i}: `;
        if (stepType === 0) {
          logMessage += `ORIGIN at (${x.toFixed(0)}, ${y.toFixed(0)})`;
        } else if (stepType === 1) {
          logMessage += `NEAREST point on segment at (${x.toFixed(0)}, ${y.toFixed(0)})`;
        } else if (stepType === 2) {
          logMessage += `ENDPOINT (start of chain) at (${x.toFixed(0)}, ${y.toFixed(0)})`;
        } else if (stepType === 3) {
          logMessage += `CHAIN point #${i - 2} at (${x.toFixed(0)}, ${y.toFixed(0)})`;
        } else if (stepType === 9) {
          logMessage += `CLOSED/CYCLE at (${x.toFixed(0)}, ${y.toFixed(0)}) ✓`;
        }

        console.log(`%c${logMessage}`, `color: ${stepColor}; font-weight: ${stepType === 0 || stepType === 1 || stepType === 2 ? 'bold' : 'normal'}`);

        // Render circle
        const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
        circle.setAttribute('cx', x);
        circle.setAttribute('cy', y);
        circle.setAttribute('fill', 'none');
        circle.setAttribute('stroke-width', 2);

        if (stepType === 0) {
          circle.setAttribute('stroke', '#ff8c00');  // orange
          circle.setAttribute('r', 5);
        } else if (stepType === 1) {
          circle.setAttribute('stroke', '#00bfff');  // cyan
          circle.setAttribute('r', 5);
        } else if (stepType === 2) {
          circle.setAttribute('stroke', '#ff1493');  // magenta
          circle.setAttribute('r', 5);
        } else if (stepType === 3) {
          // Alternate green/yellow
          const isGreen = (i - 2) % 2 === 0;
          circle.setAttribute('stroke', isGreen ? '#00ff00' : '#ffff00');
          circle.setAttribute('r', 4);
        } else if (stepType === 9) {
          circle.setAttribute('stroke', '#ff0000');  // red (closed)
          circle.setAttribute('r', 6);
        }

        fillTraceLayer.appendChild(circle);
      }

      console.log(`%c[Fill Trace] Complete`, 'color: #059669; font-weight: bold; font-size: 12px');
      
      // Log fill walk debug details
      logFillWalkDebug();
      // Log raw outgoing half-edges for the current node
      logNodeOutgoing();
      // Log duplicate node keys if any
      logNodeAudit();
      
      // Log adjacency debug (CCW half-edge walk info)
      logAdjacencyDebug();
    }

    function logAdjacencyDebug() {
      if (!wasm) return;
      
      if (typeof wasm.editor_adjacency_debug_len_f32 !== 'function' || 
          typeof wasm.editor_adjacency_debug_ptr_f32 !== 'function') {
        return;
      }
      
      const len = wasm.editor_adjacency_debug_len_f32();
      const ptr = wasm.editor_adjacency_debug_ptr_f32();
      
      if (len <= 5 || !ptr) {
        return;
      }
      
      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      const numEdges = Math.floor(arr[0]);
      
      if (numEdges <= 0) {
        return;
      }
      
      // Buffer format: [num_edges, cur.x, cur.y, prev.x, prev.y, (repeated) from.x, from.y, to.x, to.y, seg_id, he_idx]
      const curX = arr[1];
      const curY = arr[2];
      const prevX = arr[3];
      const prevY = arr[4];
      
      console.log(`%c[Adjacency] Junction (${curX.toFixed(0)}, ${curY.toFixed(0)}) - ${numEdges} outgoing edge(s)`, 
                  'color: #d946a6; font-weight: bold; font-size: 11px');
      
      let idx = 5;
      for (let i = 0; i < numEdges && idx + 5 < len; i++) {
        const fromX = arr[idx];
        const fromY = arr[idx + 1];
        const toX = arr[idx + 2];
        const toY = arr[idx + 3];
        const segId = Math.floor(arr[idx + 4]);
        const heIdx = Math.floor(arr[idx + 5]);
        idx += 6;
        
        const isReverse = toX === prevX && toY === prevY;
        const style = isReverse ? 'color: #ef4444; text-decoration: underline;' : 'color: #06b6d4;';
        
        console.log(
          `%c  Edge ${i}: (${fromX.toFixed(0)}, ${fromY.toFixed(0)}) → (${toX.toFixed(0)}, ${toY.toFixed(0)}) | seg=${segId}, he=${heIdx}${isReverse ? ' [REVERSE]' : ''}`,
          style
        );
      }
    }

    function logFillWalkDebug() {
      if (!wasm) return;
      
      if (typeof wasm.editor_fill_walk_debug_len_f32 !== 'function' ||
          typeof wasm.editor_fill_walk_debug_ptr_f32 !== 'function') {
        console.warn('[FillWalk] Functions not available');
        return;
      }
      
      const len = wasm.editor_fill_walk_debug_len_f32();
      const ptr = wasm.editor_fill_walk_debug_ptr_f32();
      
      if (len <= 0 || !ptr) {
        return;
      }
      
      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      let idx = 0;
      let block = 0;

      while (idx + 6 < len) {
        const stepIndex = Math.floor(arr[idx]);
        const curX = arr[idx + 1];
        const curY = arr[idx + 2];
        const prevX = arr[idx + 3];
        const prevY = arr[idx + 4];
        const curDeg = Math.floor(arr[idx + 5]);
        const candCount = Math.floor(arr[idx + 6]);
        idx += 7;

        console.log(`%c[Step ${stepIndex}] Node (${curX.toFixed(1)}, ${curY.toFixed(1)}) deg=${curDeg} from (${prevX.toFixed(1)}, ${prevY.toFixed(1)}) | ${candCount} cand(s)`, 
                    'color: #9333ea; font-weight: bold; font-size: 11px');

        for (let i = 0; i < candCount && idx + 5 < len; i++) {
          const nextX = arr[idx];
          const nextY = arr[idx + 1];
          const nextDeg = Math.floor(arr[idx + 2]);
          const isDead = Math.floor(arr[idx + 3]) === 1;
          const angle = arr[idx + 4];
          const chosen = Math.floor(arr[idx + 5]) === 1;
          idx += 6;

          const symbol = chosen ? '✓' : ' ';
          const style = chosen
            ? 'color: #22c55e; font-weight: bold;'
            : (isDead ? 'color: #ef4444;' : 'color: #6b7280;');

          console.log(
            `%c  [${symbol}] -> (${nextX.toFixed(1)}, ${nextY.toFixed(1)}) deg=${nextDeg} dead=${isDead ? 1 : 0} angle=${angle.toFixed(3)}`,
            style
          );
        }

        block += 1;
      }

      if (block === 0) {
        console.log('[FillWalk] Buffer empty');
      }
    }

    function logNodeOutgoing() {
      if (!wasm ||
          typeof wasm.editor_node_outgoing_len_f32 !== 'function' ||
          typeof wasm.editor_node_outgoing_ptr_f32 !== 'function') {
        return;
      }

      const len = wasm.editor_node_outgoing_len_f32();
      const ptr = wasm.editor_node_outgoing_ptr_f32();
      if (!ptr || len < 8) {
        return;
      }

      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      const curId = Math.floor(arr[0]);
      const prevId = Math.floor(arr[1]);
      const curX = arr[2];
      const curY = arr[3];
      const prevX = arr[4];
      const prevY = arr[5];
      const outgoingCount = Math.floor(arr[6]);
      const candidateCount = Math.floor(arr[7]);

      console.log(`%c[Outgoing] Node ${curId} at (${curX.toFixed(0)}, ${curY.toFixed(0)}) from prev ${prevId} (${prevX.toFixed(0)}, ${prevY.toFixed(0)}) | outgoing=${outgoingCount} | candidates=${candidateCount}`,
                  'color: #0ea5e9; font-weight: bold; font-size: 11px');

      let idx = 8;
      for (let i = 0; i < outgoingCount && idx + 8 < len; i++) {
        const heIdx = Math.floor(arr[idx]);
        const fromId = Math.floor(arr[idx + 1]);
        const toId = Math.floor(arr[idx + 2]);
        const fromX = arr[idx + 3];
        const fromY = arr[idx + 4];
        const toX = arr[idx + 5];
        const toY = arr[idx + 6];
        const angle = arr[idx + 7];
        const isBack = Math.floor(arr[idx + 8]) === 1;
        idx += 9;

        const style = isBack ? 'color: #ef4444;' : 'color: #10b981;';
        console.log(
          `%c  he=${heIdx} ${fromId}->${toId}  (${fromX.toFixed(0)},${fromY.toFixed(0)}) → (${toX.toFixed(0)},${toY.toFixed(0)}) | angle=${angle.toFixed(2)} ${isBack ? '[BACK]' : ''}`,
          style
        );
      }
    }

    function logNodeAudit() {
      if (!wasm ||
          typeof wasm.editor_node_audit_len_f32 !== 'function' ||
          typeof wasm.editor_node_audit_ptr_f32 !== 'function') {
        return;
      }

      const len = wasm.editor_node_audit_len_f32();
      const ptr = wasm.editor_node_audit_ptr_f32();
      if (!ptr || len < 1) {
        return;
      }

      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      const groupCount = Math.floor(arr[0]);
      if (groupCount <= 0) {
        return;
      }

      console.log(`%c[NodeAudit] Found ${groupCount} duplicate key group(s)`, 'color: #f97316; font-weight: bold; font-size: 11px');
      let idx = 1;
      for (let g = 0; g < groupCount && idx + 2 < len; g++) {
        const kx = arr[idx]; const ky = arr[idx + 1]; const cnt = Math.floor(arr[idx + 2]); idx += 3;
        const pts = [];
        for (let i = 0; i < cnt && idx + 2 < len; i++) {
          const id = Math.floor(arr[idx]); const x = arr[idx + 1]; const y = arr[idx + 2];
          pts.push(`#${id} (${x.toFixed(2)}, ${y.toFixed(2)})`);
          idx += 3;
        }
        console.log(`  key=(${kx},${ky}) count=${cnt}: ${pts.join(' | ')}`);
      }
    }

    function renderGraphDebug() {
      if (!wasm || !graphDebugMode) {
        graphDebugLayer.replaceChildren();
        return;
      }

      if (typeof wasm.editor_export_graph_debug_len_f32 !== 'function' ||
          typeof wasm.editor_export_graph_debug_ptr_f32 !== 'function') {
        console.warn('[GraphDebug] Functions not available');
        return;
      }

      const len = wasm.editor_export_graph_debug_len_f32();
      const ptr = wasm.editor_export_graph_debug_ptr_f32();
      if (!ptr || len < 1) {
        graphDebugLayer.replaceChildren();
        return;
      }

      const arr = new Float32Array(wasm.memory.buffer, ptr, len);
      let idx = 0;
      const segCount = Math.floor(arr[idx++] || 0);
      const segFragments = [];
      const nodeFragments = [];

      for (let i = 0; i < segCount && idx + 4 < len; i++) {
        const ax = arr[idx++]; const ay = arr[idx++]; const bx = arr[idx++]; const by = arr[idx++]; const segId = arr[idx++];
        const hue = (segId * 137.508) % 360;
        const color = `hsl(${hue}, 70%, 50%)`;

        const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
        line.setAttribute('x1', ax); line.setAttribute('y1', ay);
        line.setAttribute('x2', bx); line.setAttribute('y2', by);
        line.setAttribute('stroke', color);
        line.setAttribute('stroke-width', 3);
        line.setAttribute('stroke-linecap', 'round');
        segFragments.push(line);

        // Arrow at midpoint pointing A->B
        const midX = (ax + bx) * 0.5;
        const midY = (ay + by) * 0.5;
        const angle = Math.atan2(by - ay, bx - ax);
        const size = 6;
        const leftAngle = angle + Math.PI * 0.85;
        const rightAngle = angle - Math.PI * 0.85;
        const p1x = midX;
        const p1y = midY;
        const p2x = midX + Math.cos(leftAngle) * size;
        const p2y = midY + Math.sin(leftAngle) * size;
        const p3x = midX + Math.cos(rightAngle) * size;
        const p3y = midY + Math.sin(rightAngle) * size;

        const arrow = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
        arrow.setAttribute('points', `${p1x},${p1y} ${p2x},${p2y} ${p3x},${p3y}`);
        arrow.setAttribute('fill', color);
        arrow.setAttribute('opacity', '0.8');
        segFragments.push(arrow);

        const label = document.createElementNS('http://www.w3.org/2000/svg', 'text');
        label.setAttribute('x', midX);
        label.setAttribute('y', midY - 4);
        label.setAttribute('text-anchor', 'middle');
        label.setAttribute('dominant-baseline', 'central');
        label.setAttribute('fill', 'white');
        label.setAttribute('stroke', 'black');
        label.setAttribute('stroke-width', '0.75');
        label.setAttribute('font-size', '10');
        label.textContent = Math.floor(segId).toString();
        segFragments.push(label);
      }

      if (idx < len) {
        const nodeCount = Math.floor(arr[idx++] || 0);
        for (let i = 0; i < nodeCount && idx + 2 < len; i++) {
          const nx = arr[idx++]; const ny = arr[idx++]; const nodeId = arr[idx++];
          const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
          circle.setAttribute('cx', nx);
          circle.setAttribute('cy', ny);
          circle.setAttribute('r', 3);
          circle.setAttribute('fill', 'black');
          nodeFragments.push(circle);

          const label = document.createElementNS('http://www.w3.org/2000/svg', 'text');
          label.setAttribute('x', nx + 5);
          label.setAttribute('y', ny - 5);
          label.setAttribute('fill', 'black');
          label.setAttribute('font-size', '10');
          label.setAttribute('stroke', 'white');
          label.setAttribute('stroke-width', '0.5');
          label.textContent = `#${Math.floor(nodeId)}`;
          nodeFragments.push(label);
        }
      }

      graphDebugLayer.replaceChildren(...segFragments, ...nodeFragments);
    }

    /**
     * Allocates from the end of memory buffer
     * @param {string} str - string to encode
     * @returns {{ptr: number, len: number}} memory pointer and length
     */
    function encodeStringToWasm(str) {
      const buffer = new TextEncoder().encode(str);
      const ptr = wasm.memory.buffer.byteLength - 256;
      const view = new Uint8Array(wasm.memory.buffer, ptr, buffer.length);
      view.set(buffer);
      return { ptr, len: buffer.length };
    }

    canvas.addEventListener('pointerdown', (evt) => {
      evt.preventDefault();
      if (!wasm) return;
      
      // Track metrics + freeze detection
      metrics.pointerDownCount++;
      metrics.lastPointerId = evt.pointerId;
      metrics.lastX = evt.clientX;
      metrics.lastY = evt.clientY;
      metrics.lastPressure = evt.pressure >= 0 ? evt.pressure : -1;
      metrics.lastPointerType = evt.pointerType || '-';
      metrics.pointerState = 'down';
      metrics.currentTool = fillMode ? 'fill' : 'draw';
      metrics.lastEvtType = 'pdown';
      metrics.lastEvtTs = performance.now();
      
      const point = toSvgPoint(evt);
      eventRing.add(`EVT pdown id=${evt.pointerId} x=${point.x.toFixed(0)} y=${point.y.toFixed(0)}`);
      
      canvas.setPointerCapture(evt.pointerId);

      if (fillMode) {
        // Stay in fill mode after filling
        
        // Use dispatcher for fill action (handles debug rendering internally)
        dispatch({
          type: "Fill",
          data: {x: point.x, y: point.y, color: fillColor.value}
        }, {source: "user"});
      } else {
        startPoint = point;
        dragging = true;
        if (showLines) {
          preview.classList.add('active');
        }
        preview.setAttribute('x1', startPoint.x);
        preview.setAttribute('y1', startPoint.y);
        preview.setAttribute('x2', startPoint.x);
        preview.setAttribute('y2', startPoint.y);
      }
    });

    /**
     * Handle pointer move for line preview and debug overlay
     * @param {PointerEvent} evt
     */
    canvas.addEventListener('pointermove', (evt) => {
      // Track metrics + freeze detection
      metrics.pointerMoveCount++;
      metrics.lastX = evt.clientX;
      metrics.lastY = evt.clientY;
      metrics.lastPressure = evt.pressure >= 0 ? evt.pressure : -1;
      metrics.lastPointerId = evt.pointerId;
      metrics.lastEvtType = 'pmove';
      metrics.lastEvtTs = performance.now();
      
      const pos = toSvgPoint(evt);
      
      // Update debug overlay anytime if enabled (not just during drawing)
      if (debugMode && wasm) {
        wasm.editor_nearest(pos.x, pos.y);
        updateDebugOverlay(pos.x, pos.y);
      }
      
      // Only update preview if actively drawing
      if (!dragging) return;
      preview.setAttribute('x2', pos.x);
      preview.setAttribute('y2', pos.y);
    });

    /**
     * Update debug overlay with nearest point and line
     * @param {number} px - cursor x
     * @param {number} py - cursor y
     */
    function updateDebugOverlay(px, py) {
      const debugLen = wasm.editor_debug_len_f32();
      const debugPtr = wasm.editor_debug_ptr_f32();

      if (debugLen <= 0) {
        debugNearestLine.classList.remove('active');
        debugNearestPoint.classList.remove('active');
        debugRay.classList.remove('active');
        return;
      }

      const debugArr = new Float32Array(wasm.memory.buffer, debugPtr, debugLen);
      const hitFlag = debugArr[0];

      if (hitFlag < 0.5) {
        debugNearestLine.classList.remove('active');
        debugNearestPoint.classList.remove('active');
        debugRay.classList.remove('active');
        return;
      }

      const seg_x1 = debugArr[1];
      const seg_y1 = debugArr[2];
      const seg_x2 = debugArr[3];
      const seg_y2 = debugArr[4];
      const qx = debugArr[5];
      const qy = debugArr[6];

      // Update nearest line segment
      debugNearestLine.setAttribute('x1', seg_x1);
      debugNearestLine.setAttribute('y1', seg_y1);
      debugNearestLine.setAttribute('x2', seg_x2);
      debugNearestLine.setAttribute('y2', seg_y2);
      debugNearestLine.classList.add('active');

      // Update nearest point
      debugNearestPoint.setAttribute('cx', qx);
      debugNearestPoint.setAttribute('cy', qy);
      debugNearestPoint.classList.add('active');

      // Update ray from cursor to nearest point
      debugRay.setAttribute('x1', px);
      debugRay.setAttribute('y1', py);
      debugRay.setAttribute('x2', qx);
      debugRay.setAttribute('y2', qy);
      debugRay.classList.add('active');
    }

    canvas.addEventListener('pointerup', endDrag);
    canvas.addEventListener('pointercancel', (evt) => {
      metrics.pointerCancelCount++;
      metrics.pointerState = 'cancel';
      metrics.lastEvtType = 'pcancel';
      metrics.lastEvtTs = performance.now();
      metrics.lastPointerId = evt.pointerId;
      eventRing.add(`EVT pcancel id=${evt.pointerId}`);
      dragging = false;
      preview.classList.remove('active');
    });
    
    canvas.addEventListener('pointerleave', () => {
      dragging = false;
      preview.classList.remove('active');
      if (debugMode) {
        debugNearestLine.classList.remove('active');
        debugNearestPoint.classList.remove('active');
        debugRay.classList.remove('active');
      }
    });

    /**
     * Handle end of drag - add line to editor
     * @param {PointerEvent} evt
     */
    function endDrag(evt) {
      metrics.pointerUpCount++;
      metrics.pointerState = 'up';
      metrics.lastEvtType = 'pup';
      metrics.lastEvtTs = performance.now();
      metrics.lastPointerId = evt.pointerId;
      
      if (!dragging || !wasm) return;
      dragging = false;
      preview.classList.remove('active');
      const end = toSvgPoint(evt);
      console.log(`Line added: (${startPoint.x.toFixed(1)},${startPoint.y.toFixed(1)}) -> (${end.x.toFixed(1)},${end.y.toFixed(1)})`);
      
      // Use dispatcher for recording support
      dispatch({
        type: "AddLine",
        data: {x1: startPoint.x, y1: startPoint.y, x2: end.x, y2: end.y}
      }, {source: "user"});
      
      console.log('Total lines now:', wasm.editor_line_count());
    }

    fillBtn.addEventListener('click', () => {
      fillMode = !fillMode;
      fillBtn.classList.toggle('active');
      if (!fillMode) canvas.style.cursor = 'default';
      else canvas.style.cursor = 'crosshair';
    });

    fillColor.addEventListener('change', () => {
      if (wasm) {
        dispatch({
          type: "SetFillColor",
          data: {color: fillColor.value}
        }, {source: "user"});
      }
      // Auto-enable fill mode when color is changed
      if (!fillMode) {
        fillMode = true;
        fillBtn.classList.add('active');
        canvas.style.cursor = 'crosshair';
      }
    });

    debugBtn.addEventListener('click', () => {
      dispatch({
        type: "ToggleDebug",
        data: {debug: !debugMode}
      }, {source: "user"});
    });

    graphDebugBtn.addEventListener('click', () => {
      graphDebugMode = !graphDebugMode;
      graphDebugBtn.classList.toggle('active');
      if (graphDebugMode) {
        renderGraphDebug();
      } else {
        graphDebugLayer.replaceChildren();
      }
    });

    toggleLinesBtn.addEventListener('click', () => {
      dispatch({
        type: "ToggleShowLines",
        data: {show: !showLines}
      }, {source: "user"});
    });

    undoBtn.addEventListener('click', () => {
      if (wasm) {
        dispatch({type: "Undo", data: {}}, {source: "user"});
      }
    });

    clearBtn.addEventListener('click', () => {
      if (wasm) {
        dispatch({type: "Clear", data: {}}, {source: "user"});
      }
    });

    cleanBtn.addEventListener('click', () => {
      if (wasm) {
        dispatch({type: "Clean", data: {}}, {source: "user"});
      }
    });

    addFrameBtn.addEventListener('click', () => {
      if (!wasm) return;
      
      // Use full viewBox dimensions (0,0 to maxX, maxY)
      const minX = viewBox.x;
      const minY = viewBox.y;
      const maxX = viewBox.x + viewBox.width;
      const maxY = viewBox.y + viewBox.height;
      
      // Create 4 corners at exact viewBox boundaries (clockwise)
      const worldCorners = [
        {x: minX, y: minY},  // top-left
        {x: maxX, y: minY},  // top-right
        {x: maxX, y: maxY},  // bottom-right
        {x: minX, y: maxY}   // bottom-left
      ];
      
      // Dispatch AddFrame action
      dispatch({
        type: "AddFrame",
        data: {corners: worldCorners}
      }, {source: "user"});
      
      console.log('[Frame] Added frame at viewBox boundaries', worldCorners);
    });

    // PNG Export button handler
    exportPngBtn.addEventListener('click', () => {
      exportAsPNG();
    });

    // Escape key handler - turn off fill, debug, and graph modes
    document.addEventListener('keydown', (evt) => {
      if (evt.key === 'Escape') {
        // Turn off fill mode
        if (fillMode) {
          fillMode = false;
          fillBtn.classList.remove('active');
          canvas.style.cursor = 'default';
        }
        
        // Turn off debug mode
        if (debugMode) {
          debugMode = false;
          debugBtn.classList.remove('active');
          if (wasm) wasm.editor_set_debug(false);
          clearDebugLayers();
        }
        
        // Turn off graph debug mode
        if (graphDebugMode) {
          graphDebugMode = false;
          graphDebugBtn.classList.remove('active');
          if (wasm) wasm.editor_set_graph_debug(false);
          graphDebugLayer.replaceChildren();
        }
      }
    });

  // Load and initialize WASM module
  console.log('[WASM] Loading module...');
  wasmRaw = await initWasm();
  console.log('[WASM] Module loaded successfully');
  
  // Create wrapper object with instrumented functions
  wasm = { memory: wasmRaw.memory };
  
  // Wrap all editor_* functions for instrumentation
  for (const key in wasmRaw) {
    const value = wasmRaw[key];
    if (typeof value === 'function' && key.startsWith('editor_')) {
      wasm[key] = wasmWrapper._wrapCall(key, value.bind(wasmRaw));
    } else {
      // Copy non-function properties as-is
      wasm[key] = value;
    }
  }
  
  wasm.editor_init();
  
  // Initialize fill color
  const colorHex = fillColor.value;
  const colorBytes = new TextEncoder().encode(colorHex);
  const colorPtr = wasm.memory.buffer.byteLength - 256;
  const colorView = new Uint8Array(wasm.memory.buffer, colorPtr, colorBytes.length);
  colorView.set(colorBytes);
  wasm.editor_set_fill_color(colorPtr, colorBytes.length);
  
  // Update viewBox to match viewport size for accurate coordinate mapping
  function updateViewBox() {
    const width = window.innerWidth;
    const height = window.innerHeight;
    canvas.setAttribute('viewBox', `0 0 ${width} ${height}`);
    // Update the cached viewBox reference
    const vb = canvas.viewBox.baseVal;
    viewBox.x = vb.x;
    viewBox.y = vb.y;
    viewBox.width = vb.width;
    viewBox.height = vb.height;
  }
  
  updateViewBox();
  window.addEventListener('resize', updateViewBox);
  
  renderFromWasm();
  
  // Start metrics update loops
  let lastSlowUpdate = 0;
  
  function metricsLoop() {
    const now = performance.now();
    metrics.lastRafTs = now; // FREEZE DETECTION: UI thread heartbeat
    
    // Fast tick (every frame): timing and FPS
    const dt = metrics.lastFrameTime ? now - metrics.lastFrameTime : 16.7;
    metrics.frameDt = Math.round(dt);
    metrics.lastFrameTime = now;
    
    if (dt > 50) {
      metrics.longFrameCount++;
      eventRing.add(`WARN long frame ${dt.toFixed(0)}ms`);
    }
    
    // Rolling FPS calculation
    metrics.frameDtSum += dt;
    metrics.frameCount++;
    if (metrics.frameCount >= 10) {
      const avgDt = metrics.frameDtSum / metrics.frameCount;
      metrics.fps = Math.round(1000 / avgDt);
      metrics.frameDtSum = 0;
      metrics.frameCount = 0;
    }
    
    // Reset per-second counters
    metrics.reset();
    
    // Slow tick (every 250ms): update display and heavier metrics
    if (now - lastSlowUpdate > 250) {
      updateDebugMetrics();
      lastSlowUpdate = now;
    }
    
    requestAnimationFrame(metricsLoop);
  }
  
  requestAnimationFrame(metricsLoop);
  
  console.log('[Rustroke] Ready!');

}

// Start the application
main().catch(err => {
  console.error('[Rustroke] Fatal error:', err);
  document.body.innerHTML = '<h1>Failed to load application</h1><pre>' + err.stack + '</pre>';
});
