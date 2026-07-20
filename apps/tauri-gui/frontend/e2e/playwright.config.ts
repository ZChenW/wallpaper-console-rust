import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { resolveSystemChromiumExecutable } from './resolveChromiumExecutable.mjs';

const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const frontendDir = path.resolve(e2eDir, '..');
const systemChromium = resolveSystemChromiumExecutable();

const noProxy = [process.env.NO_PROXY, process.env.no_proxy, '127.0.0.1', 'localhost']
  .filter(Boolean)
  .join(',');

// Bypass proxies for the local mock Vite server (test runner + webServer child).
process.env.NO_PROXY = noProxy;
process.env.no_proxy = noProxy;

const systemBrowserUse = systemChromium
  ? {
      // Prefer a local Chromium/Chrome binary. launchOptions is required so the
      // Playwright Test runner does not fall back to chromium-headless-shell.
      executablePath: systemChromium,
      launchOptions: {
        executablePath: systemChromium,
      },
    }
  : {};

export default defineConfig({
  testDir: '.',
  // System Chromium + mock Vite is more stable under serial workers.
  workers: systemChromium ? 1 : undefined,
  fullyParallel: false,
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
    ...systemBrowserUse,
  },
  projects: [
    {
      name: 'desktop',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1440, height: 900 },
        ...systemBrowserUse,
      },
    },
    {
      name: 'compact',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 390, height: 844 },
        ...systemBrowserUse,
      },
    },
  ],
});
