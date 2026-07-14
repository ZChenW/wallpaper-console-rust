import type { CSSProperties } from 'react';

export type FirstRunSuggestion =
  | {
    readonly kind: 'directory';
    readonly label: string;
    readonly path: string;
  }
  | {
    readonly kind: 'wallpaperEngine';
    readonly roots?: readonly string[];
  };

export interface FirstRunSuggestionsProps {
  readonly suggestions: readonly FirstRunSuggestion[];
  readonly onAddDirectory: (path: string) => void;
  readonly onScanWallpaperEngine: () => void;
}

const sectionStyle: CSSProperties = {
  display: 'grid',
  width: 'min(44rem, 100%)',
  gap: '0.75rem',
  textAlign: 'start',
};

const headingStyle: CSSProperties = {
  margin: 0,
  fontSize: '0.95rem',
};

const noteStyle: CSSProperties = {
  margin: 0,
  fontSize: '0.78rem',
  opacity: 0.72,
};

const listStyle: CSSProperties = {
  display: 'grid',
  gap: '0.6rem',
  margin: 0,
  padding: 0,
  listStyle: 'none',
};

const suggestionStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) auto',
  alignItems: 'center',
  gap: '0.6rem 1rem',
  padding: '0.75rem',
  border: '1px solid color-mix(in srgb, currentColor 13%, transparent)',
  borderRadius: '0.65rem',
  background: 'color-mix(in srgb, CanvasText 3%, Canvas)',
};

const contentStyle: CSSProperties = {
  display: 'grid',
  minWidth: 0,
  gap: '0.2rem',
};

const labelStyle: CSSProperties = {
  fontSize: '0.87rem',
  fontWeight: 650,
};

const pathStyle: CSSProperties = {
  overflowWrap: 'anywhere',
  font: '0.7rem/1.4 ui-monospace, SFMono-Regular, Consolas, monospace',
  opacity: 0.7,
};

const rootsStyle: CSSProperties = {
  display: 'grid',
  gap: '0.15rem',
  margin: '0.2rem 0 0',
  paddingInlineStart: '1.1rem',
};

const buttonStyle: CSSProperties = {
  minHeight: '2.15rem',
  padding: '0.35rem 0.7rem',
  border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
  borderRadius: '0.5rem',
  background: 'color-mix(in srgb, AccentColor 14%, transparent)',
  color: 'inherit',
  cursor: 'pointer',
  font: 'inherit',
  fontSize: '0.78rem',
  whiteSpace: 'nowrap',
};

function usableSuggestions(
  suggestions: readonly FirstRunSuggestion[],
): FirstRunSuggestion[] {
  return suggestions.filter((suggestion) => (
    suggestion.kind === 'wallpaperEngine'
    || (suggestion.label.trim().length > 0 && suggestion.path.trim().length > 0)
  ));
}

export function FirstRunSuggestions({
  suggestions,
  onAddDirectory,
  onScanWallpaperEngine,
}: FirstRunSuggestionsProps) {
  const visibleSuggestions = usableSuggestions(suggestions);
  if (visibleSuggestions.length === 0) return null;

  return (
    <section aria-labelledby="first-run-suggestions-title" style={sectionStyle}>
      <h3 id="first-run-suggestions-title" style={headingStyle}>Suggested sources</h3>
      <p style={noteStyle}>Nothing is scanned until you confirm a suggestion.</p>
      <ul aria-label="Detected wallpaper source suggestions" style={listStyle}>
        {visibleSuggestions.map((suggestion, index) => {
          if (suggestion.kind === 'directory') {
            const label = suggestion.label.trim();
            const path = suggestion.path.trim();
            return (
              <li key={`directory:${path}`} style={suggestionStyle}>
                <div style={contentStyle}>
                  <span style={labelStyle}>{label}</span>
                  <code style={pathStyle}>{path}</code>
                </div>
                <button
                  aria-label={`Add ${label} as a wallpaper source`}
                  data-first-run-action="add-directory"
                  onClick={() => onAddDirectory(path)}
                  style={buttonStyle}
                  type="button"
                >
                  Add {label}
                </button>
              </li>
            );
          }

          const roots = (suggestion.roots ?? [])
            .map((root) => root.trim())
            .filter(Boolean);
          return (
            <li key={`wallpaper-engine:${index}`} style={suggestionStyle}>
              <div style={contentStyle}>
                <span style={labelStyle}>Wallpaper Engine</span>
                <span style={noteStyle}>Wallpaper Engine content was detected.</span>
                {roots.length > 0 ? (
                  <ul aria-label="Detected Wallpaper Engine roots" style={rootsStyle}>
                    {roots.map((root) => <li key={root}><code style={pathStyle}>{root}</code></li>)}
                  </ul>
                ) : null}
              </div>
              <button
                aria-label="Confirm Wallpaper Engine scan"
                data-first-run-action="scan-wallpaper-engine"
                onClick={onScanWallpaperEngine}
                style={buttonStyle}
                type="button"
              >
                Scan Wallpaper Engine
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

export default FirstRunSuggestions;
