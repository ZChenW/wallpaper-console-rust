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
    <section aria-labelledby="first-run-suggestions-title" className="first-run-suggestions">
      <h3 className="first-run-suggestions__heading" id="first-run-suggestions-title">
        Suggested sources
      </h3>
      <p className="first-run-suggestions__note">
        Nothing is scanned until you confirm a suggestion.
      </p>
      <ul aria-label="Detected wallpaper source suggestions" className="first-run-suggestions__list">
        {visibleSuggestions.map((suggestion, index) => {
          if (suggestion.kind === 'directory') {
            const label = suggestion.label.trim();
            const path = suggestion.path.trim();
            return (
              <li className="first-run-suggestions__item" key={`directory:${path}`}>
                <div className="first-run-suggestions__content">
                  <span className="first-run-suggestions__label">{label}</span>
                  <code className="first-run-suggestions__path">{path}</code>
                </div>
                <button
                  aria-label={`Add ${label} as a wallpaper source`}
                  className="first-run-suggestions__button"
                  data-first-run-action="add-directory"
                  onClick={() => onAddDirectory(path)}
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
            <li className="first-run-suggestions__item" key={`wallpaper-engine:${index}`}>
              <div className="first-run-suggestions__content">
                <span className="first-run-suggestions__label">Wallpaper Engine</span>
                <span className="first-run-suggestions__note">
                  Wallpaper Engine content was detected.
                </span>
                {roots.length > 0 ? (
                  <ul
                    aria-label="Detected Wallpaper Engine roots"
                    className="first-run-suggestions__roots"
                  >
                    {roots.map((root) => (
                      <li key={root}><code className="first-run-suggestions__path">{root}</code></li>
                    ))}
                  </ul>
                ) : null}
              </div>
              <button
                aria-label="Confirm Wallpaper Engine scan"
                className="first-run-suggestions__button"
                data-first-run-action="scan-wallpaper-engine"
                onClick={onScanWallpaperEngine}
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
