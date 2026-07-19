# Flow Hover Indicator Binding Design

## Goal

Unify the left Flow index rail's hover background and black vertical rule into one visual pointer without changing browsing, selection, centering, apply, or enhanced-preview behavior.

## Chosen Interaction

- When the pointer hovers a wallpaper in either the index rail or preview stream, the matching index entry shows both the hover background and black vertical rule.
- While hover is active, the previously centered entry keeps its centered typography but temporarily hides its vertical rule, so only one rule is visible.
- When the pointer leaves the interactive Flow items, hover presentation clears and the vertical rule returns to the centered entry.
- Keyboard focus, clicking, scrolling, selection, current-wallpaper state, and Apply behavior remain unchanged.

## Alternatives Considered

1. Keep the two indicators separate. This preserves current behavior but leaves two competing attention markers.
2. Bind only the visual indicators. This is the chosen approach because it clarifies focus without triggering application state or rendering work.
3. Make hover center or select the wallpaper. This was rejected because incidental pointer movement would scroll Flow, activate preview work, and undo the recent responsiveness improvements.

## Implementation Shape

The existing DOM-only hover controller remains the single source of transient hover presentation. Each index list item receives the same wallpaper ID already present on its button, allowing the controller to set `data-hovered` on the list item, button, and matching preview item in one bounded update.

CSS owns the binding:

- A hovered index item receives the black start border.
- While the Flow root has `data-hovering`, a centered index item without `data-hovered` has a transparent start border.
- With no active hover, the existing centered-item border rule remains authoritative.

No React hover state, geometry read, scroll call, selection callback, or media activation is added.

## Accessibility and Responsive Behavior

- The treatment remains pointer-only, matching the existing hover semantics; keyboard focus indicators are not replaced.
- Reduced-motion behavior is unchanged because the binding uses the existing short color transition and introduces no movement animation.
- Compact layouts that hide the index list receive no new visible behavior.
- Forced-colors behavior must preserve a single visible rule using the existing Flow state styling.

## Verification

- Unit-test the hover DOM controller with matching index-item, index-button, and preview nodes; verify stale markers clear and the centered state is not mutated.
- Update CSS contract tests to require the hovered-rule selector and the centered-rule suppression selector.
- Update the existing Flow Playwright hover test to verify that exactly one index item owns the hover marker during hover and that the marker returns to the centered item after pointer exit.
- Run frontend typecheck, unit tests, smoke tests, the Library performance gate, and the repository verification command.

## Acceptance Criteria

- Hover background and black vertical rule always identify the same wallpaper while hovering.
- Only one black vertical rule is visible at a time.
- Leaving hover restores the rule to the centered wallpaper.
- Hover never scrolls, selects, applies, or starts enhanced media.
- Editorial layout, typography, filters, and three-column composition remain unchanged.
