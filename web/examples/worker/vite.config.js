import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 3002,
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
  optimizeDeps: {
    exclude: ['entidb_wasm'],
  },
  worker: {
    format: 'es',
  },
});
