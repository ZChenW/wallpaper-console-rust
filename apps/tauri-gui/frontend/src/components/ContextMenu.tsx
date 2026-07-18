import { useEffect, useRef, type KeyboardEvent } from 'react';
import type { ContextAction } from './WallpaperGrid';
import { emitFeedback } from '../events/appEvents';

interface Props {
  x: number;
  y: number;
  path: string;
  actions: ContextAction[];
  onClose: () => void;
}

export type ContextMenuKeyResolution =
  | { type: 'focus'; index: number }
  | {
      type: 'close';
      restoreFocus: boolean;
      deferUntilAfterTraversal: boolean;
    }
  | null;

export function resolveContextMenuKey(
  key: string,
  currentIndex: number,
  itemCount: number,
): ContextMenuKeyResolution {
  if (key === 'Escape') {
    return {
      type: 'close',
      restoreFocus: true,
      deferUntilAfterTraversal: false,
    };
  }
  if (key === 'Tab') {
    return {
      type: 'close',
      restoreFocus: false,
      deferUntilAfterTraversal: true,
    };
  }
  if (itemCount <= 0) return null;

  switch (key) {
    case 'ArrowDown':
      return { type: 'focus', index: currentIndex < 0 ? 0 : (currentIndex + 1) % itemCount };
    case 'ArrowUp':
      return {
        type: 'focus',
        index: currentIndex < 0 ? itemCount - 1 : (currentIndex - 1 + itemCount) % itemCount,
      };
    case 'Home':
      return { type: 'focus', index: 0 };
    case 'End':
      return { type: 'focus', index: itemCount - 1 };
    default:
      return null;
  }
}

interface ContextMenuPositionInput {
  x: number;
  y: number;
  menuWidth: number;
  menuHeight: number;
  viewportWidth: number;
  viewportHeight: number;
  margin?: number;
}

export function clampContextMenuPosition({
  x,
  y,
  menuWidth,
  menuHeight,
  viewportWidth,
  viewportHeight,
  margin = 8,
}: ContextMenuPositionInput): { left: number; top: number } {
  const maximumLeft = Math.max(margin, viewportWidth - menuWidth - margin);
  const maximumTop = Math.max(margin, viewportHeight - menuHeight - margin);
  return {
    left: Math.min(Math.max(x, margin), maximumLeft),
    top: Math.min(Math.max(y, margin), maximumTop),
  };
}

export default function ContextMenu({ x, y, path, actions, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const restoreFocusOnCloseRef = useRef(true);
  const returnFocusRef = useRef<HTMLElement | null>(
    typeof document !== 'undefined'
      && document.activeElement instanceof HTMLElement
      && document.activeElement !== document.body
      ? document.activeElement
      : null,
  );

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  useEffect(() => () => {
    if (!restoreFocusOnCloseRef.current) return;
    const returnFocus = returnFocusRef.current;
    if (!returnFocus?.isConnected) return;

    const activeElement = document.activeElement;
    const focusMovedOutside = activeElement instanceof HTMLElement
      && activeElement !== document.body
      && !ref.current?.contains(activeElement);
    if (!focusMovedOutside) returnFocus.focus();
  }, []);

  useEffect(() => {
    const menu = ref.current;
    if (!menu) return undefined;

    const placeWithinViewport = () => {
      const bounds = menu.getBoundingClientRect();
      const { left, top } = clampContextMenuPosition({
        x,
        y,
        menuWidth: bounds.width,
        menuHeight: bounds.height,
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight,
      });
      menu.style.left = `${left}px`;
      menu.style.top = `${top}px`;
    };

    placeWithinViewport();
    window.addEventListener('resize', placeWithinViewport);
    return () => window.removeEventListener('resize', placeWithinViewport);
  }, [actions, x, y]);

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const menuItems = Array.from(
      ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [],
    );
    const currentIndex = menuItems.indexOf(document.activeElement as HTMLButtonElement);
    const resolution = resolveContextMenuKey(event.key, currentIndex, menuItems.length);
    if (!resolution) return;

    if (resolution.type === 'close' && resolution.deferUntilAfterTraversal) {
      restoreFocusOnCloseRef.current = resolution.restoreFocus;
      window.requestAnimationFrame(onClose);
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    if (resolution.type === 'close') {
      restoreFocusOnCloseRef.current = resolution.restoreFocus;
      onClose();
      return;
    }

    menuItems.forEach((item, index) => {
      item.tabIndex = index === resolution.index ? 0 : -1;
    });
    menuItems[resolution.index]?.focus();
  };

  return (
    <div
      aria-label="Wallpaper actions"
      className="context-menu"
      onKeyDown={handleKeyDown}
      ref={ref}
      role="menu"
      style={{ left: x, top: y }}
    >
      {actions.map((a, index) => (
        <button
          autoFocus={index === 0}
          key={a.label}
          className={a.danger ? 'danger' : ''}
          role="menuitem"
          tabIndex={index === 0 ? 0 : -1}
          onClick={async () => {
            try {
              await a.action(path, returnFocusRef.current);
            } catch (e) {
              emitFeedback({ state: 'error', label: a.label, detail: String(e) });
            } finally {
              onClose();
            }
          }}
        >
          {a.label}
        </button>
      ))}
    </div>
  );
}
