import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Vite now only builds the web view; the shell is Rust (src-tauri).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  // Tauri targets a fixed, modern webview - no legacy transpilation needed.
  build: {
    target: 'chrome110',
    minify: 'esbuild',
    sourcemap: false,
  },
})
