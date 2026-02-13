# Recording & Playback Feature

## Quick Start

1. Open `web/index.html` in your browser
2. Click **Record** button
3. Draw some lines and create fills
4. Click **Stop**
5. Click **Play** to replay your actions!

## Features

### Recording
- Records all user actions with millisecond timestamps
- Captures: lines, fills, undo, clear, and UI state changes
- Console logs each recorded action

### Playback
- Replays actions with original timing
- Automatically clears canvas before playback
- Disables canvas input during playback (visual indication)
- Can be stopped mid-playback

### Export/Import
- **Export**: Copies recording JSON to clipboard
- **Import**: Load recording from JSON (paste into prompt)
- Share recordings with others!

## Sample Recording

A sample recording is included in `sample_recording.json`. To use it:
1. Click **Import**
2. Paste the contents of `sample_recording.json`
3. Click **Play**

The sample draws two triangles and fills them with different colors.

## Action Types Supported

1. **AddLine** - Draw a line
   ```json
   {"t": 0, "type": "AddLine", "data": {"x1": 100, "y1": 100, "x2": 200, "y2": 200}}
   ```

2. **Fill** - Fill a region
   ```json
   {"t": 500, "type": "Fill", "data": {"x": 150, "y": 150, "color": "#ff0000"}}
   ```

3. **Undo** - Undo last action
   ```json
   {"t": 1000, "type": "Undo", "data": {}}
   ```

4. **Clear** - Clear canvas
   ```json
   {"t": 1500, "type": "Clear", "data": {}}
   ```

5. **SetFillColor** - Change fill color
   ```json
   {"t": 2000, "type": "SetFillColor", "data": {"color": "#00ff00"}}
   ```

6. **ToggleShowLines** - Show/hide lines
   ```json
   {"t": 2500, "type": "ToggleShowLines", "data": {"show": false}}
   ```

7. **ToggleDebug** - Enable/disable debug mode
   ```json
   {"t": 3000, "type": "ToggleDebug", "data": {"debug": true}}
   ```

## Console Logs

Open browser console to see detailed logs:
- `[Record]` prefix for recorded actions
- `[Playback]` prefix during playback
- Each action shows timestamp and data

## Button States

- **Record**: Disabled during recording/playback
- **Stop**: Only enabled during recording/playback
- **Play**: Disabled if no recording or during recording/playback
- **Clear Rec**: Disabled if no recording or during recording/playback
- **Export**: Disabled if no recording
- **Import**: Always enabled (except during recording/playback)

## Tips

1. **Deterministic playback**: Recordings use world coordinates, so they replay identically
2. **Fill colors**: Each Fill action stores its color, ensuring accurate reproduction
3. **Timing**: Timestamps are relative to recording start (milliseconds)
4. **Interrupt**: Click Stop during playback to cancel remaining actions
5. **Share**: Export recordings as JSON and share with others!

## Technical Details

- Pure JavaScript/HTML (no external dependencies)
- All actions route through central `dispatch()` function
- Playback uses `setTimeout` for timing
- Canvas disabled during playback (pointer-events: none, opacity: 0.7)
- Recording state isolated from playback state


---

## New: Animated Line Strokes During Playback! ✨

### Visual Playback
Playback now shows **real-time line drawing animation**:
- Lines animate from start to end point (200ms per line)
- Smooth 60fps animation using requestAnimationFrame
- Uses the preview line element (same as live drawing)

### Before vs After
- **Before:** Lines appeared instantly during playback
- **After:** Watch lines being drawn stroke by stroke!

### How It Works
1. During playback, each AddLine action triggers animation
2. Preview line shows from (x1,y1) to (x2,y2)
3. Linear interpolation over 200ms
4. Line added to canvas when animation completes
5. Next action begins (respecting original timing)

### Performance
- 60fps smooth animation
- No frame drops or stuttering
- Async/await ensures proper sequencing
- Stop button halts mid-animation

### Try It
1. Import the sample_recording.json
2. Click Play
3. Watch the lines and fills appear as if drawn by hand!

The sample now includes 3 shapes (triangle, rectangle, triangle) with animated strokes.

