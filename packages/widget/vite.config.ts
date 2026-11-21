import { defineConfig } from 'vite';

// `vite build` emits the ESM client library, `vite build --mode widget` emits the
// self contained IIFE that the server embeds and serves as /widget.js.
export default defineConfig(({ mode }) => {
  const widget = mode === 'widget';
  return {
    build: {
      target: 'es2020',
      emptyOutDir: !widget,
      sourcemap: false,
      minify: true,
      lib: widget
        ? { entry: 'src/widget.ts', formats: ['iife'], name: 'EarfulWidget', fileName: () => 'widget.js' }
        : { entry: 'src/index.ts', formats: ['es'], fileName: () => 'client.js' },
      rollupOptions: widget ? {} : { external: [/^@noble\/hashes/] },
    },
    test: {
      environment: 'happy-dom',
    },
  };
});
