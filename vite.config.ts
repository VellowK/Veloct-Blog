import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The Tauri shell reads `frontendDist: "../dist-ui"` from `src-tauri/tauri.conf.json`, so the build
// output lives there rather than in the default `dist`. The dev server is pinned to the port the
// shell's `devUrl` names, because Tauri waits for exactly that origin.
export default defineConfig({
  plugins: [react()],
  // `clearScreen: false` keeps Rust compiler output visible while `tauri dev` runs both.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: '127.0.0.1',
  },
  build: {
    outDir: 'dist-ui',
    emptyOutDir: true,
    // WebView2 on Windows is current enough for the modern target, and a lower target would only make
    // the output larger for no benefit.
    target: 'chrome110',
    sourcemap: false,
  },
});
