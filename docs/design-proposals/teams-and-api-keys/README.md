# Teams list and API key dialogs: design proposals

Status: proposal (no live pages changed)
Source UI:
- `crates/admin-ui/web/src/routes/identity/-teams/teams-list.tsx`
- `crates/admin-ui/web/src/routes/api-keys/-components.tsx` and `-scope-fields.tsx`

Reference UI: `routes/observability/-attribution/tabs/pull-requests-tab.tsx` (the PR ledger) and the
Client config pill group in `routes/models.tsx`.

The mockups are static HTML using the admin UI's dark-theme tokens from `styles/globals.css`.
They load `shared.js` and `styles.css`, so serve the folder rather than opening the files directly:

```bash
python3 -m http.server 4321 --directory docs/design-proposals
```

Then open `http://localhost:4321/teams-and-api-keys/teams.html` and `.../api-keys.html`.
Both pages are interactive: expand rows, filter by tag, and switch the pill groups.

## 1. Teams: the PR-ledger anatomy (`teams.html`)

| Today | Proposed |
|---|---|
| Two always-visible buttons per row (Edit team, Add members) | One `⋯` menu in a pinned right column, next to the expand chevron |
| A "Show 1 member" button opens the roster | The whole row toggles, as on the PR ledger; members show as an avatar stack |
| Roster is a stack of bordered cards, each with Transfer and Remove | A nested table in a `bg-muted/40` detail panel, with a per-member `⋯` menu |
| "No admins" repeated as text on most rows | A "Without admins" option in the Filter menu |
| Tags wrap under the team name and can push the row to 3 to 4 lines | A Tags column: two chips, then `+N` with a popover holding every tag |
| No search or filtering | Search, a Filter menu (tag pairs, no admins), and a status segmented toggle |

The layout uses only fields `AdminTeamManagementView` already returns (name, key, status, admins,
members, tags), so it needs no API change.

### Tags

Teams carry up to five `key:value` tags. Options considered:

- **A. Two chips, then `+N` (recommended).** One line every time. A chip shows the key muted and
  the value bright in one pill, and clicking it filters by that pair. The popover lists every tag.
- **B. Count only** (`5 tags`). The quietest row, but you can't scan it for a key.
- **C. Promote a tag key to a column**, using a Columns popover like the one on Models. Strong for
  org-wide keys such as `cost-center`; a follow-up once tag keys settle.

### ReUI fit

The ledger is a hand-composed `Table`, the same as the PR tab, so it stays one container family
(Card plus bordered table). To move to ReUI, the nearest references are:

- [`c-data-grid-8`](https://reui.io/preview/base/components/c-data-grid-8) (free): expandable rows.
- [`data-grid-expansion-1`](https://reui.io/preview/base/data-grid-expansion-1) (Pro): parent rows
  with nested sub-rows, plus a Filters bar.
- [`data-grid-base-6`](https://reui.io/preview/base/data-grid-base-6) (Pro): a members grid with
  avatar cells and hover actions.

`@reui/filters` would own the tag filter chips that the mockup draws by hand.

## 2. API key dialogs: one button-group style (`api-keys.html`)

Model grant mode already uses the Client config pills (`ToggleGroup variant="checked" size="sm"
spacing={1}`, from commit `5ea97678`). Capabilities and MCP servers in `-scope-fields.tsx` still
use the default variant. Its "on" state is `bg-muted`, which is darker than the dialog's popover
surface, so neither option looks selected.

Proposed:

1. Capabilities and MCP servers switch to the `checked` pill group. This is the core fix.
2. Owner type becomes a pill group too (optional; today it's a two-option Select).
3. Fields group under Owner and Access section labels.
4. The secret row puts the masked key beside outline `sm` Reveal and Copy buttons, as in the Models
   toolbar, rather than full-width buttons that move after reveal.
5. Lifecycle becomes one row with a `destructive sm` Revoke button, which still opens the confirm.
6. Both dialogs end with a fixed bordered footer: secondary Cancel and the primary action.

Leading icons on the pills are optional. Drop them to match Model grant mode exactly.
