/** The context-menu gesture shared by both library views. */
export function isContextMenuKey(key: string, shiftKey = false): boolean {
  return key === 'ContextMenu' || (key === 'F10' && shiftKey);
}
