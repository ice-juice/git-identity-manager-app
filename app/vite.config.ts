import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, loadEnv } from 'vite'
import { displayNameFor, isEnglishLang } from '../scripts/brand.mjs'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "")
  const raw = String(env.VITE_APP_LANG || "zh").trim().toLowerCase()
  const lang = isEnglishLang(raw) ? "en" : "zh"
  const displayName = displayNameFor(lang)

  return {
    plugins: [react(), tailwindcss()],
    envDir: process.cwd(),
    define: {
      "import.meta.env.VITE_APP_LANG": JSON.stringify(lang),
      "import.meta.env.VITE_APP_NAME": JSON.stringify(displayName),
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
