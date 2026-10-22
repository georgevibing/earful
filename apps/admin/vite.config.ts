import { defineConfig } from 'vitest/config';

const backend = 'http://127.0.0.1:8080';

export default defineConfig({
  base: '/admin/',
  oxc: {
    jsx: { runtime: 'automatic', importSource: 'preact' },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    assetsInlineLimit: 0,
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    proxy: {
      '/api': backend,
      '/s': backend,
      '/widget.js': backend,
      '/healthz': backend,
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
