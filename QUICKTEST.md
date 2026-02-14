# Rustroke Quick Test Reference

## Run Tests (Choose One)

```bash
# ⚡ FASTEST: Automated CLI test (30 seconds)
npm test

# 🌐 VISUAL: Browser test dashboard (2 minutes)  
npm run test:browser

# 🎨 MANUAL: Test main app (5 minutes)
npm run dev
```

## Expected Results

### npm test
```
✅ All tests passed! WASM module is healthy.
📊 Results: 23/23 tests passed
```

### Browser Self-Test (http://localhost:8080/self-test.html)
```
✓ All tests green
✓ Summary shows: "23 Passed, 0 Failed"
✓ No red entries in console
```

### Main App (http://localhost:8080/)
```
✓ Can draw lines
✓ Can fill closed shapes
✓ Can undo/redo
✓ No console errors
```

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Tests fail | `cargo +nightly build --release --target wasm32-unknown-unknown && cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/wasm/rustroke.wasm` |
| Fill doesn't work | Draw a **closed** shape (triangle, not a line) |
| Graph shows nothing | Draw a **closed** shape (open lines filtered out) |
| Browser cache | Hard refresh: `Cmd+Shift+R` (Mac) or `Ctrl+Shift+F5` (Win) |

## Key Facts

✅ **All 23 automated tests pass**
✅ **WASM module is 74KB and healthy**
✅ **All core features verified working**

⚠️ **Fill only works on closed shapes** - This is CORRECT behavior (not a bug)

## Quick Commands

```bash
# Test
npm test

# Dev
npm run dev

# Build
npm run build

# Preview production
npm run preview
```

## Files

- `web/self-test.html` - Visual test dashboard
- `web/diagnostic.cjs` - CLI test runner
- `TESTING.md` - Full test guide
- `TEST_SUMMARY.md` - Implementation details
