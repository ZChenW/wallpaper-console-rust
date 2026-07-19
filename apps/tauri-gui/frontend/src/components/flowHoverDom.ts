export interface FlowHoverElement {
  removeAttribute(name: string): void;
  setAttribute(name: string, value: string): void;
}

export interface FlowHoverRoot extends FlowHoverElement {
  querySelectorAll(selector: string): Iterable<FlowHoverElement>;
  toggleAttribute(name: string, force: boolean): boolean;
}

export function applyFlowHover(
  root: FlowHoverRoot | null,
  wallpaperId: number | null,
): void {
  if (root === null) return;

  root.toggleAttribute('data-hovering', wallpaperId !== null);
  for (const element of root.querySelectorAll('[data-hovered]')) {
    element.removeAttribute('data-hovered');
  }
  if (wallpaperId === null) return;

  for (const element of root.querySelectorAll(`[data-wallpaper-id="${wallpaperId}"]`)) {
    element.setAttribute('data-hovered', 'true');
  }
}
