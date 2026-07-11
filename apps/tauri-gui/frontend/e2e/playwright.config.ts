import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const frontendDir = path.resolve(e2eDir, '..');

const noProxy = [process.env.NO_PROXY, process.env.no_proxy, '127.0.0.1', 'localhost']
  .filter(Boolean)
  .join(',');

// Bypass proxies for the local mock Vite server (test runner + webServer child).
process.env.NO_PROXY = noProxy;
process.env.no_proxy = noProxy;

export default defineConfig({
  testDir: '.',
  timeout: 30_000,
  webServer: {
    command: 'npx vite --config vite.mock.config.ts --host 127.0.0.1 --port 4174',
    cwd: frontendDir,
    url: 'http://127.0.0.1:4174',
    reuseExistingServer: !process.env.CI,
    env: {
      ...process.env,
      NO_PROXY: noProxy,
      no_proxy: noProxy,
    },
  },
  use: {
    baseURL: 'http://127.0.0.1:4174',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    { name: 'compact', use: { ...devices['Desktop Chrome'], viewport: { width: 390, height: 844 } } },
  ],
});
