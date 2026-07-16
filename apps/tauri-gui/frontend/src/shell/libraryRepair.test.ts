import assert from 'node:assert/strict';
import test from 'node:test';

import type { CommandResult } from '../api/types.ts';
import {
  classifyLibraryVerification,
  faultAfterVerification,
  shouldVerifyLibraryIntegrity,
  verifyLibraryIntegrity,
} from './libraryRepair.ts';
import { effectiveSourceFilter } from './singlePageShellModel.ts';

const ok: CommandResult = {
  success: true,
  stdout: 'ok',
  stderr: '',
  exitCode: 0,
};

const corrupt: CommandResult = {
  success: false,
  stdout: 'quick_check failed',
  stderr: 'database disk image is malformed',
  exitCode: 1,
  error: {
    kind: 'sqlite_integrity',
    message: 'Library database integrity check failed',
    detail: 'page 12 is malformed',
    recoverable: true,
    suggestion: 'Run repair',
  },
};

const unavailable: CommandResult = {
  success: false,
  stdout: '',
  stderr: 'database is locked',
  exitCode: 1,
  error: {
    kind: 'command_failed',
    message: 'database is locked',
    recoverable: true,
  },
};

test('verification distinguishes healthy, confirmed corruption, and unavailable storage', () => {
  assert.deepEqual(classifyLibraryVerification(ok), { status: 'ok' });

  const corruptOutcome = classifyLibraryVerification(corrupt);
  assert.equal(corruptOutcome.status, 'corrupt');
  assert.equal(corruptOutcome.status === 'corrupt' && corruptOutcome.fault.message,
    'Library database needs repair');
  assert.match(corruptOutcome.status === 'corrupt' ? corruptOutcome.fault.technicalDetails : '',
    /integrity check failed/i);
  assert.match(corruptOutcome.status === 'corrupt' ? corruptOutcome.fault.technicalDetails : '',
    /page 12 is malformed/);

  const unavailableOutcome = classifyLibraryVerification(unavailable);
  assert.equal(unavailableOutcome.status, 'unavailable');
  assert.match(unavailableOutcome.status === 'unavailable'
    ? unavailableOutcome.technicalDetails
    : '', /database is locked/i);
});

test('verification transport failure does not pretend database corruption was detected', async () => {
  const transport = await verifyLibraryIntegrity({
    sqliteVerify: async () => { throw new Error('bridge offline'); },
  });
  assert.equal(transport.status, 'unavailable');
  assert.match(transport.status === 'unavailable' ? transport.technicalDetails : '', /bridge offline/);
  assert.deepEqual(await verifyLibraryIntegrity({ sqliteVerify: async () => ok }), { status: 'ok' });
  assert.deepEqual(
    await verifyLibraryIntegrity({ sqliteVerify: async () => corrupt }),
    classifyLibraryVerification(corrupt),
  );
});

test('a confirmed repair fault persists through unavailable checks until verification succeeds', () => {
  const corruptOutcome = classifyLibraryVerification(corrupt);
  assert.equal(corruptOutcome.status, 'corrupt');
  if (corruptOutcome.status !== 'corrupt') return;

  const unavailableOutcome = classifyLibraryVerification(unavailable);
  assert.deepEqual(
    faultAfterVerification(corruptOutcome.fault, unavailableOutcome),
    corruptOutcome.fault,
  );
  assert.equal(faultAfterVerification(corruptOutcome.fault, { status: 'ok' }), null);
});

test('integrity verification runs for storage errors and only an unfiltered confirmed empty library', () => {
  const defaults = {
    browserLoadError: false,
    sourceLoadError: false,
    sourceCount: 1,
    emptyConfirmed: true,
    sourceFilter: { kind: 'all' as const },
    typeFilter: 'usable' as const,
    favoritesOnly: false,
    search: '',
  };

  assert.equal(shouldVerifyLibraryIntegrity(defaults), true);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, search: '   ' }), true);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, browserLoadError: true }), true);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, sourceLoadError: true }), true);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, sourceCount: 0 }), false);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, emptyConfirmed: false }), false);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, search: 'missing' }), false);
  assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, favoritesOnly: true }), false);
  assert.equal(shouldVerifyLibraryIntegrity({
    ...defaults,
    sourceFilter: { kind: 'source', sourceId: 7 },
  }), false);
  for (const typeFilter of ['image', 'gif', 'video', 'weScene', 'unsupported'] as const) {
    assert.equal(shouldVerifyLibraryIntegrity({ ...defaults, typeFilter }), false);
  }
});

test('source error forces effective filter to all so verification still runs with specific source preference', () => {
  // When the source catalog has an error, effectiveSourceFilter forces 'all'
  // regardless of the persisted preference. shouldVerifyLibraryIntegrity must
  // receive the effective filter so it triggers verification even when the
  // user has a source-specific filter persisted.
  const sourceError = 'source database unavailable';
  const persistedFilter = { kind: 'source' as const, sourceId: 7 };
  const effective = effectiveSourceFilter(persistedFilter, sourceError);
  assert.deepEqual(effective, { kind: 'all' as const });

  // With the persisted source-specific filter, verification would be skipped.
  assert.equal(shouldVerifyLibraryIntegrity({
    browserLoadError: false,
    sourceLoadError: true,  // because source catalog errored
    sourceCount: 3,
    emptyConfirmed: true,
    sourceFilter: persistedFilter,   // what the old code passed
    typeFilter: 'usable',
    favoritesOnly: false,
    search: '',
  }), true); // still true because sourceLoadError short-circuits

  // But when there's NO sourceLoadError and NO browserLoadError,
  // verification depends on the sourceFilter. The old code would
  // skip verification with a source-specific filter:
  assert.equal(shouldVerifyLibraryIntegrity({
    browserLoadError: false,
    sourceLoadError: false,
    sourceCount: 3,
    emptyConfirmed: true,
    sourceFilter: persistedFilter,  // old code: skipped verification
    typeFilter: 'usable',
    favoritesOnly: false,
    search: '',
  }), false); // WRONG: should verify after source recovery

  // With the effective filter (forced to 'all' due to source error),
  // verification fires correctly:
  assert.equal(shouldVerifyLibraryIntegrity({
    browserLoadError: false,
    sourceLoadError: false,
    sourceCount: 3,
    emptyConfirmed: true,
    sourceFilter: effective,  // forced to 'all' by effectiveSourceFilter
    typeFilter: 'usable',
    favoritesOnly: false,
    search: '',
  }), true); // CORRECT: effective filter enables verification
});
