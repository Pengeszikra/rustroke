export default {
  root: './web',
  base: './',
  build: {
    target: 'esnext',
    outDir: '../dist',
    emptyOutDir: true,
    assetsInclude: ['**/*.wasm']
  },
  assetsInclude: ['**/*.wasm'],
  server: {
    port: 8080
  }
}
