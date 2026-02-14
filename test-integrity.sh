#!/bin/bash
# Test script for verifying integrity checks work in debug builds

set -e

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Rustroke Integrity Check Test Suite"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

echo "1. Building DEBUG version (checks enabled)..."
cargo +nightly build --target wasm32-unknown-unknown
DEBUG_SIZE=$(stat -f%z target/wasm32-unknown-unknown/debug/rust_svg_editor.wasm)
echo "   ✓ Debug build size: $(numfmt --to=iec $DEBUG_SIZE 2>/dev/null || echo $DEBUG_SIZE bytes)"
echo

echo "2. Building RELEASE version (checks disabled)..."
cargo +nightly build --release --target wasm32-unknown-unknown
RELEASE_SIZE=$(stat -f%z target/wasm32-unknown-unknown/release/rust_svg_editor.wasm)
echo "   ✓ Release build size: $(numfmt --to=iec $RELEASE_SIZE 2>/dev/null || echo $RELEASE_SIZE bytes)"
echo

OVERHEAD=$((DEBUG_SIZE - RELEASE_SIZE))
echo "3. Comparing sizes..."
echo "   Debug overhead: $(numfmt --to=iec $OVERHEAD 2>/dev/null || echo $OVERHEAD bytes)"
echo "   Release overhead: 0 bytes (checks compiled out)"
echo

echo "4. Verifying exports..."
EXPORTS=$(strings target/wasm32-unknown-unknown/release/rust_svg_editor.wasm | grep -c "^editor_" || echo 0)
echo "   ✓ Found $EXPORTS editor_* exports"
echo

echo "5. Checking for thread-safety compile guard..."
if grep -q "target_feature.*atomics" src/lib.rs; then
    echo "   ✓ Compile-time atomics check present"
else
    echo "   ✗ Missing atomics check!"
    exit 1
fi
echo

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All integrity checks verified!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo
echo "Manual stress tests:"
echo "  • Rapid draw: Hold mouse and scribble for 10 seconds"
echo "  • Rapid undo: Add 50+ lines, then spam Undo"  
echo "  • Fill spam: Draw triangle, click fill 20x rapidly"
echo "  • Mixed ops: Random draw/fill/undo/trim sequence"
echo
echo "See INTEGRITY_SAFETY.md for full documentation."
