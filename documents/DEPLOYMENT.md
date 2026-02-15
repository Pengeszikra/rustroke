# Rustroke - Deployment Guide

## 🚀 Quick Deploy to Vercel

[![Deploy with Vercel](https://vercel.com/button)](https://vercel.com/new/clone?repository-url=https://github.com/YOUR_USERNAME/rustroke)

This project is configured for **static deployment** - Vercel will **NOT** compile Rust.

## 📦 Pre-built WASM

The repository contains pre-compiled WASM binaries in `web/wasm/`:
- `rustroke.wasm` - WebAssembly module
- `rustroke.js` - WASM loader

## 🔧 Local Development

### Prerequisites
- Node.js 18+ (for Vite)
- Rust nightly (only if rebuilding WASM)

### Setup

```bash
# Install dependencies
npm install

# Start dev server
npm run dev
```

Open http://localhost:8080

### Build for Production

```bash
# Build static site
npm run build

# Preview production build
npm run preview
```

## 🔨 Rebuilding WASM (Optional)

Only needed if you modify Rust code:

```bash
# Build WASM
./build_and_serve.sh

# Or manually:
cargo +nightly build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/wasm/rustroke.wasm
```

**Important:** Commit the updated `web/wasm/rustroke.wasm` after rebuilding.

## 📁 Project Structure

```
rustroke/
├── src/              # Rust source code
├── web/              # Frontend (served by Vite)
│   ├── wasm/         # Pre-built WASM (committed to repo)
│   │   ├── rustroke.wasm
│   │   └── rustroke.js
│   ├── index.html
│   └── main.js
├── vite.config.js    # Vite configuration
├── vercel.json       # Vercel deployment config
└── package.json      # Node dependencies (vite only)
```

## ⚙️ How Deployment Works

1. **Vercel runs:** `npm install && npm run build`
2. **Vite bundles:** HTML, JS, CSS, and copies WASM
3. **Output:** Static files in `dist/`
4. **Vercel serves:** Static site from `dist/`

**No Rust compilation happens on Vercel!**

## 🎯 Features

- ✅ Vector drawing engine
- ✅ Fill algorithm with graph traversal  
- ✅ Record/playback with JSON export
- ✅ Frame tool
- ✅ Undo/redo support
- ✅ Debug visualization modes

## 📝 Environment Variables

None required - fully static deployment.

## 🐛 Troubleshooting

### WASM fails to load in production

Make sure `web/wasm/rustroke.wasm` is committed to the repository:
```bash
git add web/wasm/rustroke.wasm
git commit -m "Add WASM binary"
```

### Build fails on Vercel

Check that `package.json` has the correct build command:
```json
{
  "scripts": {
    "build": "vite build"
  }
}
```

### Blank page after deployment

Open browser console. If you see CORS errors, the WASM file may not be in `dist/`.
Check `vite.config.js` has:
```js
assetsInclude: ['**/*.wasm']
```

## 📄 License

See LICENSE file.
