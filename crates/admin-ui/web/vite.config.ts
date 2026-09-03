import { defineConfig } from 'vite'
import tsConfigPaths from 'vite-tsconfig-paths'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import autoprefixer from 'autoprefixer'
import tailwindcss from '@tailwindcss/postcss'

const port = Number(process.env.PORT ?? 3001)

export default defineConfig({
  base: '/admin/',
  css: {
    postcss: {
      plugins: [tailwindcss(), autoprefixer()],
    },
  },
  server: {
    port,
    strictPort: true,
    hmr: {
      host: 'localhost',
      port,
      clientPort: port,
    },
  },
  plugins: [tsConfigPaths(), tanstackStart(), viteReact()],
})
