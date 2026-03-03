import { defineConfig } from 'vite'
import tsConfigPaths from 'vite-tsconfig-paths'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'

export default defineConfig({
  base: '/admin/',
  server: {
    port: 3001,
    strictPort: true,
    hmr: {
      host: 'localhost',
      port: 3001,
      clientPort: 3001,
    },
  },
  plugins: [tsConfigPaths(), tanstackStart(), viteReact()],
})
