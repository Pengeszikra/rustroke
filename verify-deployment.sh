#!/usr/bin/env bash
# Verify Vercel deployment readiness

set -e

echo "🔍 Vercel Deployment Verification"
echo "=================================="
echo ""

# Check required files exist
echo "📁 Checking required files..."
REQUIRED_FILES=(
  "package.json"
  "vite.config.js"
  "vercel.json"
  "web/index.html"
  "web/main.js"
  "web/wasm/rustroke.js"
  "web/wasm/rustroke.wasm"
)

for file in "${REQUIRED_FILES[@]}"; do
  if [ -f "$file" ]; then
    echo "  ✅ $file"
  else
    echo "  ❌ $file - MISSING!"
    exit 1
  fi
done

echo ""
echo "📦 Checking WASM size..."
WASM_SIZE=$(stat -f%z web/wasm/rustroke.wasm 2>/dev/null || stat -c%s web/wasm/rustroke.wasm 2>/dev/null || echo "0")
if [ "$WASM_SIZE" -gt 10000 ]; then
  echo "  ✅ WASM file: $(numfmt --to=iec-i --suffix=B $WASM_SIZE 2>/dev/null || echo "${WASM_SIZE} bytes")"
else
  echo "  ❌ WASM file too small or missing"
  exit 1
fi

echo ""
echo "🔧 Checking package.json..."
if grep -q '"vite"' package.json; then
  echo "  ✅ Vite dependency found"
else
  echo "  ❌ Vite dependency missing"
  exit 1
fi

if grep -q '"build": "vite build"' package.json; then
  echo "  ✅ Build script correct"
else
  echo "  ❌ Build script incorrect"
  exit 1
fi

echo ""
echo "🔧 Checking vite.config.js..."
if grep -q 'assetsInclude.*wasm' vite.config.js; then
  echo "  ✅ WASM assets configured"
else
  echo "  ❌ WASM assets not configured"
  exit 1
fi

echo ""
echo "🔧 Checking vercel.json..."
if grep -q '"framework": "vite"' vercel.json; then
  echo "  ✅ Framework set to Vite"
else
  echo "  ❌ Framework not set correctly"
  exit 1
fi

echo ""
echo "📄 Checking index.html..."
if grep -q 'type="module"' web/index.html; then
  echo "  ✅ Module script tag found"
else
  echo "  ❌ Module script tag missing"
  exit 1
fi

if grep -q 'src="./main.js"' web/index.html; then
  echo "  ✅ main.js imported"
else
  echo "  ❌ main.js not imported"
  exit 1
fi

echo ""
echo "📄 Checking main.js..."
if grep -q "import initWasm from" web/main.js; then
  echo "  ✅ WASM import found"
else
  echo "  ❌ WASM import missing"
  exit 1
fi

echo ""
echo "🧪 Testing build..."
if npm run build >/dev/null 2>&1; then
  echo "  ✅ Build succeeds"
  
  if [ -f "dist/index.html" ]; then
    echo "  ✅ dist/index.html created"
  else
    echo "  ❌ dist/index.html not created"
    exit 1
  fi
  
  if ls dist/assets/*.wasm >/dev/null 2>&1; then
    echo "  ✅ WASM copied to dist/assets/"
  else
    echo "  ❌ WASM not in dist/assets/"
    exit 1
  fi
else
  echo "  ❌ Build failed"
  exit 1
fi

echo ""
echo "════════════════════════════════════"
echo "  ✅ ALL CHECKS PASSED!"
echo "════════════════════════════════════"
echo ""
echo "🚀 Ready to deploy to Vercel!"
echo ""
echo "Next steps:"
echo "  1. Push to GitHub: git push"
echo "  2. Deploy: vercel --prod"
echo "  3. Or: Connect repo at vercel.com/new"
echo ""
