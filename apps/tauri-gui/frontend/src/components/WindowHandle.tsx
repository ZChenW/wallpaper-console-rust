/**
 * The window has no decorations and is moved by the shell's top bar. A view that fills the window
 * hides that bar, so it carries this strip along its top edge instead: pressing it moves the window
 * rather than reaching the view underneath.
 */
export default function WindowHandle({ className }: { readonly className: string }) {
  return <div className={`window-handle ${className}`} data-tauri-drag-region="deep" title="Drag to move the window" />;
}
