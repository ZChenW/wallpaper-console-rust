import assert from 'node:assert/strict';
import test from 'node:test';
import { nextSelectionForClick } from './useGridSelection.ts';

function idx(paths: string[]): Map<string, number> {
  return new Map(paths.map((path, index) => [path, index]));
}

test('ctrl click toggles one path', () => {
  const result = nextSelectionForClick(['a', 'b'], new Set(), null, 'a', { ctrlKey: true }, idx(['a', 'b']));
  assert.deepEqual([...result.selectedPaths], ['a']);
  assert.equal(result.lastClickedPath, 'a');
});

test('ctrl click removes path when already selected', () => {
  const result = nextSelectionForClick(['a', 'b'], new Set(['a']), null, 'a', { ctrlKey: true }, idx(['a', 'b']));
  assert.deepEqual([...result.selectedPaths], []);
  assert.equal(result.lastClickedPath, 'a');
});

test('shift click selects inclusive range with indexed lookup', () => {
  const paths = ['a', 'b', 'c', 'd'];
  const map = idx(paths);
  const first = nextSelectionForClick(paths, new Set(), null, 'b', {}, map);
  const second = nextSelectionForClick(paths, first.selectedPaths, first.lastClickedPath, 'd', { shiftKey: true }, map);
  assert.deepEqual([...second.selectedPaths], ['b', 'c', 'd']);
});

test('plain click replaces selection with single path', () => {
  const result = nextSelectionForClick(['a', 'b'], new Set(['b']), 'b', 'a', {}, idx(['a', 'b']));
  assert.deepEqual([...result.selectedPaths], ['a']);
  assert.equal(result.lastClickedPath, 'a');
});
