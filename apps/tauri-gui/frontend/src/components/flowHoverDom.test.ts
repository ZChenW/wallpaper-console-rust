import assert from 'node:assert/strict';
import test from 'node:test';

import { applyFlowHover } from './flowHoverDom.ts';

class FakeHoverElement {
  readonly attributes = new Map<string, string>();

  removeAttribute(name: string) {
    this.attributes.delete(name);
  }

  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
}

class FakeHoverRoot extends FakeHoverElement {
  readonly elements = new Map<string, FakeHoverElement[]>();

  querySelectorAll(selector: string): FakeHoverElement[] {
    if (selector === '[data-hovered]') {
      return [...this.elements.values()]
        .flat()
        .filter((element) => element.attributes.has('data-hovered'));
    }
    return this.elements.get(selector) ?? [];
  }

  toggleAttribute(name: string, force: boolean) {
    if (force) this.attributes.set(name, '');
    else this.attributes.delete(name);
    return force;
  }
}

test('Flow hover updates matching DOM nodes and clears stale presentation without layout reads', () => {
  const root = new FakeHoverRoot();
  const stale = new FakeHoverElement();
  stale.setAttribute('data-hovered', 'true');
  const preview = new FakeHoverElement();
  const indexEntry = new FakeHoverElement();
  const indexItem = new FakeHoverElement();
  root.elements.set('[data-wallpaper-id="42"]', [preview, indexEntry, indexItem]);
  root.elements.set('stale', [stale]);

  applyFlowHover(root, 42);

  assert.equal(root.attributes.has('data-hovering'), true);
  assert.equal(stale.attributes.has('data-hovered'), false);
  assert.equal(preview.attributes.get('data-hovered'), 'true');
  assert.equal(indexEntry.attributes.get('data-hovered'), 'true');
  assert.equal(indexItem.attributes.get('data-hovered'), 'true');

  applyFlowHover(root, null);

  assert.equal(root.attributes.has('data-hovering'), false);
  assert.equal(preview.attributes.has('data-hovered'), false);
  assert.equal(indexEntry.attributes.has('data-hovered'), false);
  assert.equal(indexItem.attributes.has('data-hovered'), false);
});
