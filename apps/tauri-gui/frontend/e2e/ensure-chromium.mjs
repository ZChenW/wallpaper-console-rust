import { constants } from 'node:fs';
import { access } from 'node:fs/promises';

import { chromium } from '@playwright/test';

import { resolveSystemChromiumExecutable } from './resolveChromiumExecutable.mjs';

const systemExecutable = resolveSystemChromiumExecutable();
if (systemExecutable) {
  console.log(`Using system Chromium/Chrome: ${systemExecutable}`);
  process.exit(0);
}

const bundledExecutable = chromium.executablePath();
try {
  await access(bundledExecutable, constants.X_OK);
  console.log(`Using Playwright-managed Chromium: ${bundledExecutable}`);
  process.exit(0);
} catch {
  console.error([
    'No usable Chromium/Chrome browser found for Playwright.',
    'Looked for a system browser (chromium/chrome) and a Playwright-managed browser.',
    'Browser-dependent smoke/E2E cannot run without downloading a browser.',
  ].join('\n'));
  process.exitCode = 1;
}
