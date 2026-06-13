import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const frontendDir = path.resolve(e2eDir, '..');
const mockConfig = path.join(frontendDir, 'vite.mock.config.ts');

process.env.NO_PROXY = [process.env.NO_PROXY, '127.0.0.1', 'localhost']
  .filter(Boolean)
  .join(',');
process.env.no_proxy = process.env.NO_PROXY;

export default defineConfig({
  testDir: '.',
  timeout: 30_000,
  webServer: {
    command: `NO_PROXY=127.0.0.1,localhost no_proxy=127.0.0.1,localhost npx vite --config ${mockConfig} --host 127.0.0.1 --port 4174`,
    url: 'http://127.0.0.1:4174',
    reuseExistingServer: !process.env.CI,
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
