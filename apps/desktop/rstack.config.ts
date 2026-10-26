import { pluginReact } from '@rsbuild/plugin-react';
import { define } from 'rstack';

define.app({
  plugins: [pluginReact()],
  source: {
    entry: {
      index: './src/main.tsx',
    },
  },
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  output: {
    distPath: {
      root: 'dist',
    },
  },
});
