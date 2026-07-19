import { spawnSync } from 'node:child_process';

const result = spawnSync(
  process.platform === 'win32' ? 'npx.cmd' : 'npx',
  ['playwright', 'test', '--config', 'e2e/playwright.config.ts', 'library-perf.spec.ts'],
  { cwd: new URL('..', import.meta.url), stdio: 'inherit', env: process.env },
);

if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
