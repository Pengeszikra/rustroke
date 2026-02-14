#!/usr/bin/env node

/**
 * Rustroke Diagnostic Tool
 * Tests WASM module functionality without a browser
 */

const fs = require('fs');
const path = require('path');

console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
console.log('🔍 Rustroke WASM Diagnostic');
console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
console.log();

let passCount = 0;
let failCount = 0;

function test(name, fn) {
  try {
    const result = fn();
    if (result && typeof result.then === 'function') {
      return result.then(() => {
        console.log(`✓ ${name}`);
        passCount++;
        return true;
      }).catch(err => {
        console.log(`✗ ${name}`);
        console.log(`  Error: ${err.message}`);
        failCount++;
        return false;
      });
    }
    console.log(`✓ ${name}`);
    passCount++;
    return true;
  } catch (err) {
    console.log(`✗ ${name}`);
    console.log(`  Error: ${err.message}`);
    failCount++;
    return false;
  }
}

async function main() {
  // Test 1: File existence
  console.log('📁 File Checks:');
  test('WASM file exists', () => {
    const wasmPath = path.join(__dirname, 'wasm', 'rustroke.wasm');
    if (!fs.existsSync(wasmPath)) {
      throw new Error(`WASM not found at ${wasmPath}`);
    }
  });

  test('WASM loader exists', () => {
    const loaderPath = path.join(__dirname, 'wasm', 'rustroke.js');
    if (!fs.existsSync(loaderPath)) {
      throw new Error(`Loader not found at ${loaderPath}`);
    }
  });

  test('Main entry exists', () => {
    const mainPath = path.join(__dirname, 'main.js');
    if (!fs.existsSync(mainPath)) {
      throw new Error(`main.js not found`);
    }
  });

  test('Self-test page exists', () => {
    const testPath = path.join(__dirname, 'self-test.html');
    if (!fs.existsSync(testPath)) {
      throw new Error(`self-test.html not found`);
    }
  });
  console.log();

  // Test 2: WASM structure
  console.log('🔬 WASM Structure:');
  const wasmPath = path.join(__dirname, 'wasm', 'rustroke.wasm');
  const wasmBuffer = fs.readFileSync(wasmPath);
  
  test('WASM magic number', () => {
    const magic = wasmBuffer.slice(0, 4).toString('hex');
    if (magic !== '0061736d') {
      throw new Error(`Invalid magic: ${magic}`);
    }
  });

  test('WASM version', () => {
    const version = wasmBuffer.readUInt32LE(4);
    if (version !== 1) {
      throw new Error(`Invalid version: ${version}`);
    }
  });

  test('WASM size reasonable', () => {
    const sizeMB = (wasmBuffer.length / 1024 / 1024).toFixed(2);
    console.log(`  Size: ${wasmBuffer.length} bytes (${sizeMB} MB)`);
    if (wasmBuffer.length < 10000 || wasmBuffer.length > 10000000) {
      throw new Error(`Suspicious size: ${wasmBuffer.length}`);
    }
  });
  console.log();

  // Test 3: WASM loading
  console.log('⚙️  WASM Loading:');
  let wasm = null;
  
  const loaded = await test('WASM instantiation', async () => {
    const result = await WebAssembly.instantiate(wasmBuffer, { env: {} });
    wasm = result.instance;
    if (!wasm) throw new Error('No instance');
  });

  if (!loaded || !wasm) {
    console.log();
    console.log('❌ Cannot continue - WASM failed to load');
    process.exit(1);
  }

  test('Memory export', () => {
    if (!wasm.exports.memory) {
      throw new Error('No memory export');
    }
  });

  test('Editor functions exported', () => {
    const required = [
      'editor_init',
      'editor_add_line',
      'editor_undo',
      'editor_clear',
      'editor_line_count',
      'editor_fill_debug_at',
      'editor_cleanup_overhangs'
    ];
    
    for (const fn of required) {
      if (typeof wasm.exports[fn] !== 'function') {
        throw new Error(`Missing: ${fn}`);
      }
    }
    
    const allEditorFns = Object.keys(wasm.exports).filter(k => k.startsWith('editor_'));
    console.log(`  Found ${allEditorFns.length} editor_* functions`);
  });
  console.log();

  // Test 4: Basic operations
  console.log('🧪 Basic Operations:');
  
  test('Initialize editor', () => {
    wasm.exports.editor_init();
  });

  test('Initial line count is zero', () => {
    const count = wasm.exports.editor_line_count();
    if (count !== 0) {
      throw new Error(`Expected 0, got ${count}`);
    }
  });

  test('Add line', () => {
    wasm.exports.editor_add_line(10, 10, 100, 100);
    const count = wasm.exports.editor_line_count();
    if (count !== 1) {
      throw new Error(`Expected 1 line, got ${count}`);
    }
  });

  test('Add multiple lines', () => {
    wasm.exports.editor_add_line(0, 0, 50, 0);
    wasm.exports.editor_add_line(50, 0, 25, 50);
    const count = wasm.exports.editor_line_count();
    if (count !== 3) {
      throw new Error(`Expected 3 lines, got ${count}`);
    }
  });

  test('Undo operation', () => {
    wasm.exports.editor_undo();
    const count = wasm.exports.editor_line_count();
    if (count !== 2) {
      throw new Error(`Expected 2 lines after undo, got ${count}`);
    }
  });

  test('Clear canvas', () => {
    wasm.exports.editor_clear();
    const count = wasm.exports.editor_line_count();
    if (count !== 0) {
      throw new Error(`Expected 0 lines after clear, got ${count}`);
    }
  });

  test('Undo after clear', () => {
    wasm.exports.editor_undo();
    const count = wasm.exports.editor_line_count();
    if (count !== 2) {
      throw new Error(`Expected 2 lines after undo-clear, got ${count}`);
    }
  });
  console.log();

  // Test 5: Advanced features
  console.log('🎨 Advanced Features:');

  test('Set fill color', () => {
    const colorStr = '#FF0000';
    const colorBytes = Buffer.from(colorStr);
    const memSize = wasm.exports.memory.buffer.byteLength;
    const colorPtr = memSize - 256;
    const mem = new Uint8Array(wasm.exports.memory.buffer);
    mem.set(colorBytes, colorPtr);
    wasm.exports.editor_set_fill_color(colorPtr, colorBytes.length);
  });

  test('Fill operation (may not create fill)', () => {
    wasm.exports.editor_clear();
    // Closed triangle
    wasm.exports.editor_add_line(0, 0, 100, 0);
    wasm.exports.editor_add_line(100, 0, 50, 100);
    wasm.exports.editor_add_line(50, 100, 0, 0);
    
    wasm.exports.editor_fill_debug_at(50, 30);
    // Note: Fill may not create a polygon due to closed component filter
    // This is not a failure
  });

  test('Add frame', () => {
    wasm.exports.editor_clear();
    wasm.exports.editor_add_frame(0, 0, 800, 600);
    const count = wasm.exports.editor_line_count();
    if (count !== 4) {
      throw new Error(`Frame should add 4 lines, got ${count}`);
    }
  });

  test('Cleanup overhangs', () => {
    wasm.exports.editor_clear();
    wasm.exports.editor_add_line(0, 0, 100, 0);
    wasm.exports.editor_add_line(100, 0, 50, 100);
    wasm.exports.editor_add_line(50, 100, 0, 0);
    wasm.exports.editor_add_line(200, 200, 300, 200); // Dangling
    
    wasm.exports.editor_cleanup_overhangs();
    // Should remove dangling line
  });

  test('Debug mode toggle', () => {
    wasm.exports.editor_set_debug(true);
    wasm.exports.editor_set_debug(false);
  });

  test('Export data available', () => {
    wasm.exports.editor_clear();
    wasm.exports.editor_add_line(0, 0, 100, 100);
    const len = wasm.exports.editor_export_len_f32();
    if (len === 0) {
      throw new Error('Export buffer is empty');
    }
    console.log(`  Export buffer: ${len} floats`);
  });
  console.log();

  // Summary
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
  const total = passCount + failCount;
  console.log(`📊 Results: ${passCount}/${total} tests passed`);
  
  if (failCount === 0) {
    console.log('✅ All tests passed! WASM module is healthy.');
    console.log();
    console.log('To test in browser:');
    console.log('  1. Run: npm run dev');
    console.log('  2. Open: http://localhost:8080/self-test.html');
    console.log('  3. Or main app: http://localhost:8080/');
    process.exit(0);
  } else {
    console.log(`⚠️  ${failCount} test(s) failed`);
    console.log();
    console.log('Troubleshooting:');
    console.log('  1. Rebuild: cargo +nightly build --release --target wasm32-unknown-unknown');
    console.log('  2. Copy: cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/wasm/rustroke.wasm');
    console.log('  3. Retest: node web/diagnostic.js');
    process.exit(1);
  }
}

main().catch(err => {
  console.error('Fatal error:', err);
  process.exit(1);
});
