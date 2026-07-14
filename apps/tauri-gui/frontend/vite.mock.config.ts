import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

const mockBridge = path.resolve(__dirname, 'src/api/mockBridge.ts');

export default defineConfig({
  root: __dirname,
  plugins: [react()],
  base: './',
  resolve: {
    alias: [
      { find: /^(?:\.\.\/)+api\/bridge(?:\.ts)?$/, replacement: mockBridge },
      { find: /^\.\/api\/bridge(?:\.ts)?$/, replacement: mockBridge },
      { find: /^src\/api\/bridge(?:\.ts)?$/, replacement: mockBridge },
    ],
  },
  build: {
    outDir: 'dist-mock',
    emptyOutDir: true,
  },
});
