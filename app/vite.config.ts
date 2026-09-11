import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, loadEnv } from 'vite'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "")
  const raw = String(env.VITE_APP_LANG || "zh").trim().toLowerCase()
  const lang = raw === "en" || raw.startsWith("en-") ? "en" : "zh"

  return {
  plugins: [react(), tailwindcss()],
  envDir: process.cwd(),
  define: {
    "import.meta.env.VITE_APP_LANG": JSON.stringify(lang),
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // Cargo 编译时会锁 target 下的 dll；Vite 监听这些文件会在 Windows 上 EBUSY 崩溃
      ignored: ['**/src-tauri/**'],
    },
  },
}
})
