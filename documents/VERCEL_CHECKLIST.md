# Vercel Deployment Checklist ✅

## Pre-Deployment Verification

### 1. Repository Structure ✓
- [x] `web/wasm/rustroke.wasm` exists and is committed
- [x] `web/wasm/rustroke.js` exists and is committed  
- [x] `web/index.html` uses module script: `<script type="module" src="./main.js"></script>`
- [x] `web/main.js` imports WASM: `import initWasm from './wasm/rustroke.js'`
- [x] `vite.config.js` exists with correct config
- [x] `vercel.json` exists
- [x] `package.json` exists with build scripts
- [x] `.gitignore` excludes `dist/`, `node_modules/`, but NOT `web/wasm/`

### 2. Local Build Test ✓
```bash
npm install
npm run build
npx serve dist
```
- [x] Build completes without errors
- [x] `dist/` directory created
- [x] WASM file copied to `dist/assets/*.wasm`
- [x] App loads in browser at http://localhost:3000
- [x] Drawing works
- [x] Fill works
- [x] Undo/redo works

### 3. WASM Loading ✓
- [x] No manual `fetch('*.wasm')` in code
- [x] Uses `import initWasm from './wasm/rustroke.js'`
- [x] Init is awaited: `wasm = await initWasm()`
- [x] WASM exports accessed correctly

### 4. Vite Configuration ✓
File: `vite.config.js`
```javascript
export default {
  root: './web',
  base: './',
  build: {
    target: 'esnext',
    outDir: '../dist',
    emptyOutDir: true,
    assetsInclude: ['**/*.wasm']
  },
  assetsInclude: ['**/*.wasm']
}
```

### 5. Vercel Configuration ✓
File: `vercel.json`
```json
{
  "framework": "vite",
  "buildCommand": "npm run build",
  "outputDirectory": "dist",
  "installCommand": "npm install"
}
```

### 6. Package.json ✓
File: `package.json`
```json
{
  "name": "rustroke",
  "version": "1.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  },
  "devDependencies": {
    "vite": "^5.0.0"
  }
}
```

## Deployment Steps

### Option 1: Deploy via Vercel CLI

```bash
# Install Vercel CLI (first time only)
npm install -g vercel

# Login to Vercel
vercel login

# Deploy
vercel

# Deploy to production
vercel --prod
```

### Option 2: Deploy via Vercel Dashboard

1. Go to https://vercel.com/new
2. Import your Git repository
3. Vercel auto-detects Vite config
4. Click "Deploy"
5. Wait 30-60 seconds
6. Visit your site URL

### Option 3: Deploy via GitHub Integration

1. Connect GitHub repo to Vercel
2. Push to main branch
3. Auto-deploys on every push
4. Production: main branch
5. Preview: PR branches

## Post-Deployment Verification

### Check Deployment Logs
1. Go to Vercel dashboard → Deployments
2. Click latest deployment
3. Check "Build" tab for errors
4. Verify "Running Build Command" succeeded
5. Verify "Uploading Build Outputs" includes WASM

### Test Production Site
```bash
# Visit your Vercel URL
https://your-project.vercel.app

# Open browser DevTools Console
# Check for:
# ✓ [Rustroke] Initializing...
# ✓ [WASM] Loading module...
# ✓ [WASM] Module loaded successfully
# ✓ [Rustroke] Ready!
```

### Functionality Test
- [ ] Page loads without errors
- [ ] Draw a line with mouse
- [ ] Draw multiple lines
- [ ] Click "Fill" button
- [ ] Click inside a closed shape → fills correctly
- [ ] Click "Undo" → removes last action
- [ ] Click "Frame" → adds border
- [ ] Click "Record" → starts recording
- [ ] Perform actions
- [ ] Click "Stop"
- [ ] Click "Play" → replays actions
- [ ] Toggle "Hide Lines" → lines disappear, fills remain
- [ ] Toggle "Debug" → shows debug overlays

## Troubleshooting

### Build fails with "Cannot find module"
**Problem:** WASM files not in repository  
**Solution:**
```bash
git add web/wasm/rustroke.wasm web/wasm/rustroke.js
git commit -m "Add WASM binaries"
git push
```

### WASM fails to load (404 error)
**Problem:** Vite not copying WASM  
**Solution:** Check `vite.config.js` has:
```javascript
assetsInclude: ['**/*.wasm']
```

### WASM loads but app doesn't work
**Problem:** WASM exports not accessible  
**Solution:** Check `web/wasm/rustroke.js` returns correct exports

### Blank page in production
**Problem:** JavaScript error  
**Solution:**
1. Open browser DevTools
2. Check Console for errors
3. Check Network tab for failed requests
4. Verify WASM file loaded (Status 200)

### Different behavior than local
**Problem:** Using wrong WASM file  
**Solution:** Rebuild WASM and commit:
```bash
./build_and_serve.sh  # or manual cargo build
git add web/wasm/rustroke.wasm
git commit -m "Update WASM"
git push
```

## Important Notes

⚠️ **NEVER add Rust to Vercel build**
- Vercel does NOT compile Rust
- WASM must be pre-built and committed
- Only frontend (Vite) builds run on Vercel

⚠️ **Always commit WASM after rebuilding**
```bash
# After modifying Rust code:
cargo +nightly build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/rust_svg_editor.wasm web/wasm/rustroke.wasm
git add web/wasm/rustroke.wasm
git commit -m "Update WASM"
```

⚠️ **Base path is relative**
- `base: './'` in vite.config.js
- Works for both root and subpath deployments

⚠️ **Module scripts only**
- No inline scripts in HTML
- All JS in external modules
- Enables proper ES6 imports

## Success Criteria

✅ Build completes in < 60 seconds  
✅ No Rust compilation on Vercel  
✅ WASM loads without CORS errors  
✅ All features work identically to local  
✅ Console shows no errors  
✅ Network tab shows WASM loaded (200)  

## Next Steps After Deployment

1. **Custom Domain** (optional)
   - Add in Vercel dashboard → Settings → Domains

2. **Environment Variables** (if needed in future)
   - Add in Vercel dashboard → Settings → Environment Variables

3. **Analytics** (optional)
   - Enable Vercel Analytics in dashboard

4. **Continuous Deployment**
   - Already enabled via GitHub integration
   - Every push to main = new deployment

---

**You're ready to deploy! 🚀**

Run: `vercel` or push to GitHub
