# Admin UI Conventions

## Cards and Page Structure
- Match existing admin page card rhythm: consistent `Card`, `CardHeader`, `CardTitle`, `CardDescription`, and `CardContent` usage.
- Avoid nested “card in a card” layouts unless the inner card is a distinct repeated item or modal surface.
- Separate unrelated page sections into separate cards. Do not place a visual wrapper behind multiple cards unless the broader page pattern already does this.
- Keep page-level backgrounds quiet; the only visual layer behind cards should usually be the body background.

## Tables
- Use the Teams/API Keys table style for admin data tables:
  - outlined table container
  - header row with muted background
  - sentence case column labels
  - muted header text color
  - left-aligned cells
  - consistent row height and vertical alignment
  - row hover state
- Prefer action buttons with consistent sizing, background, hover state, and tooltips.

## Dialogs
- Detail dialogs with multiple sections should use a sidebar navigation pattern.
- Keep section tabs such as Overview, Configuration, Tools, and Credentials inside the dialog instead of expanding page-level complexity.
- Dialog content must be width-contained:
  - use `min-w-0` on flex/grid children
  - use `max-w-full` where long content can appear
  - use `overflow-hidden` on parent panels that should not expand horizontally
  - put horizontal scrolling on the innermost long-content element, such as JSON/code blocks

## MCP Server UI

- Use Lobe mono icons for known MCP server brands where available, matching by server key/name. Keep the generic MCP mark for unknown servers.
- Do not allow brand icons to render as opaque square blocks; verify SVG/icon color behavior in dark mode.
- Tool lists should use collapsed, expandable rows rather than wide tables.
- Collapsed tool rows should show only selector, tool name, optional truncated description, status, and expand control.
- Expanded tool rows should show useful metadata and JSON schema only. Avoid noisy fields like schema hash, first seen, and last seen unless specifically needed.
- Long JSON schemas must scroll inside their own code panel and must not widen or clip the dialog.

## Regression Coverage
- Add route/component tests for interactive admin UI regressions:
  - selection state keeps actions visible
  - inactive rows cannot be selected
  - expanded rows reveal the expected details
  - hidden implementation metadata stays hidden
- For known layout regressions, add stable test IDs around critical containers and assert containment classes such as `min-w-0`, `max-w-full`, `overflow-hidden`, and inner `overflow-x-auto`.
