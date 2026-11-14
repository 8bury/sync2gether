import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The scenes load Inter from the app's own assets/fonts, one level up.
export default defineConfig({
  plugins: [react()],
  build: { target: 'es2022' },
  server: { host: '127.0.0.1', fs: { allow: ['..'] } },
})
