import { defineConfig } from 'vite'
import tsConfigPaths from 'vite-tsconfig-paths'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'

export default defineConfig({
  base: '/admin/',
  server: {
    port: 3001,
  },
  plugins: [tsConfigPaths(), tanstackStart(), viteReact()],
})
