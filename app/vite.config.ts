import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // Cargo 编译时会锁 target 下的 dll；Vite 监听这些文件会在 Windows 上 EBUSY 崩溃
      ignored: ['**/src-tauri/**'],
    },
  },
})
