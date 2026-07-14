import { constants } from 'node:fs';
import { access } from 'node:fs/promises';

import { chromium } from '@playwright/test';

const executable = chromium.executablePath();

try {
  await access(executable, constants.X_OK);
} catch {
  console.error([
    'Playwright Chromium is not installed for this package version.',
    'Run: npx playwright install chromium',
  ].join('\n'));
  process.exitCode = 1;
}
