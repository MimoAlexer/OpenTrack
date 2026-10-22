import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react(), {
    name: 'check-cascade-layer-order',
    enforce: 'post',
    generateBundle(_, bundle) {
      // Component imports must never declare their layer before the reset.
      // Check emitted CSS as well as source ordering: the bundler controls it.
      for (const asset of Object.values(bundle)) {
        if (asset.type !== 'asset' || !asset.fileName.endsWith('.css')) continue;
        const css = typeof asset.source === 'string' ? asset.source : new TextDecoder().decode(asset.source);
        const layers = [...new Set([...css.matchAll(/@layer\s+([^{};]+)[{;]/g)].flatMap(match => match[1].split(',').map(name => name.trim())))];
        if (layers.slice(0, 4).join(',') !== 'reset,theme,components,responsive') this.error('CSS layers must start with reset, theme, components, responsive. Import styles.css before component modules.');
      }
    },
  }],
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': { target: 'http://127.0.0.1:8080', changeOrigin: false } },
  },
  build: { target: 'es2022', sourcemap: true },
});
