import { defineConfig } from 'vite'
import tsConfigPaths from 'vite-tsconfig-paths'
import viteReact from '@vitejs/plugin-react'
import autoprefixer from 'autoprefixer'
import tailwindcss from '@tailwindcss/postcss'

// A separate entry keeps fixture data and design controls out of the admin bundle.
export default defineConfig({
  css: { postcss: { plugins: [tailwindcss(), autoprefixer()] } },
  plugins: [tsConfigPaths(), viteReact()],
  server: { host: '127.0.0.1', port: 4317, strictPort: true, open: false },
  build: {
    outDir: 'dist-designs',
    rolldownOptions: { input: ['designs/index.html', 'designs/toolsets.html'] },
  },
})
