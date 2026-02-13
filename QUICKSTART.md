# 🚀 Quick Start - Deploy to Vercel in 3 Steps

This project is **ready to deploy**. WASM is already built and committed.

## Step 1: Install Dependencies

```bash
npm install
```

## Step 2: Test Locally (Optional)

```bash
# Development server with hot reload
npm run dev
# → http://localhost:8080

# Production build test
npm run build
npx serve dist
# → http://localhost:3000
```

## Step 3: Deploy to Vercel

### Option A: Vercel CLI (Fastest)

```bash
npm install -g vercel
vercel login
vercel --prod
```

### Option B: GitHub Integration (Recommended)

1. Push to GitHub:
   ```bash
   git add .
   git commit -m "Ready for deployment"
   git push
   ```

2. Go to https://vercel.com/new

3. Import your repository

4. Click "Deploy" (no config needed!)

5. Done! 🎉

---

## What Happens During Deployment

```
Vercel receives push
  ↓
Runs: npm install
  ↓
Runs: npm run build
  ↓
Vite bundles HTML/JS/CSS
  ↓
Vite copies WASM from web/wasm/
  ↓
Output to dist/
  ↓
Vercel serves static files
  ↓
Your app is live! 🌐
```

**Important:** No Rust compilation happens on Vercel.  
The `.wasm` file is already built and committed in `web/wasm/`.

---

## After Deployment

✅ Visit your Vercel URL  
✅ App should load instantly  
✅ Draw lines, fill shapes, use record/playback  
✅ Check browser console for `[Rustroke] Ready!`  

---

## Updating the App

### If you change Rust code:

```bash
# 1. Rebuild WASM locally
cargo +nightly build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/wasm/rustroke.wasm

# 2. Commit and push
git add web/wasm/rustroke.wasm
git commit -m "Update WASM"
git push

# Vercel auto-deploys in ~30 seconds
```

### If you change HTML/JS/CSS:

```bash
# Just push - Vite rebuilds automatically
git add .
git commit -m "Update UI"
git push
```

---

## Need Help?

- 📖 Full deployment guide: `DEPLOYMENT.md`
- ✅ Deployment checklist: `VERCEL_CHECKLIST.md`
- 🐛 Troubleshooting: See VERCEL_CHECKLIST.md

---

## That's It!

No complex setup. No build configuration.  
Just push and deploy. 🚀
