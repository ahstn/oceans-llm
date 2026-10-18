---
name: Oceans Gateway Admin
description: Same-origin operations console for the Oceans LLM Gateway.
colors:
  frame: "#040c15"
  content-panel: "#07111c"
  card: "#0c1a29"
  floating-layer: "#0e2133"
  secondary-fill: "#102336"
  muted-layer: "#0c1f31"
  hover-wash: "#0d2841"
  table-header: "#0c1e2f"
  ink: "#e8edf3"
  soft-text: "#94a7bb"
  nav-text: "#cad2db"
  hairline: "rgb(236 240 247 / 0.08)"
  input-line: "rgb(236 240 247 / 0.12)"
  lilac: "#bb9af7"
  on-lilac: "#040c15"
  logo-ocean-blue: "#016f9f"
  logo-mist: "#b5dde5"
  wave-foam: "#d6f7f9"
  wave-deep: "#01294b"
  success-green: "#73daca"
  warning: "#eaa518"
  destructive-rose: "#fb7185"
  chart-blue: "#7aa2f7"
  chart-green: "#9ece6a"
  chart-teal: "#41a6b5"
  chart-cyan: "#7dcfff"
  chart-purple: "#9d7cd8"
  chart-other: "#737aa2"
  heatmap-empty: "#172432"
  heatmap-1: "#4d68a7"
  heatmap-2: "#6183d1"
  heatmap-3: "#7aa2f7"
  heatmap-4: "#89ddff"
typography:
  display:
    fontFamily: "Geist Variable, ui-sans-serif, sans-serif"
    fontSize: "clamp(2.25rem, 2vw + 1.5rem, 3rem)"
    fontWeight: 500
    lineHeight: 1.1
    letterSpacing: "0"
  headline:
    fontFamily: "Geist Variable, ui-sans-serif, sans-serif"
    fontSize: "1rem"
    fontWeight: 500
    lineHeight: 1.375
  body:
    fontFamily: "Geist Variable, ui-sans-serif, sans-serif"
    fontSize: "clamp(15px, 0.28vw + 14px, 16px)"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Geist Variable, ui-sans-serif, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.25
  caption:
    fontFamily: "Geist Variable, ui-sans-serif, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.333
rounded:
  sm: "calc(var(--radius) * 0.6)"
  md: "calc(var(--radius) * 0.8)"
  lg: "var(--radius)"
  xl: "calc(var(--radius) * 1.4)"
  pill: "9999px"
spacing:
  xs: "0.25rem"
  sm: "0.5rem"
  md: "1rem"
  lg: "1.5rem"
  xl: "2rem"
components:
  button-primary:
    backgroundColor: "{colors.lilac}"
    textColor: "{colors.on-lilac}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0 0.625rem"
  button-secondary:
    backgroundColor: "{colors.secondary-fill}"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0 0.625rem"
  card:
    backgroundColor: "{colors.card}"
    textColor: "{colors.ink}"
    rounded: "{rounded.xl}"
    padding: "1rem"
  input:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0.25rem 0.625rem"
---

# Design System: Oceans Gateway Admin

## 1. Overview

**Creative North Star: "The Gateway Console"**

Oceans Gateway Admin is a focused, technical operations console that should feel native to the gateway runtime. Its visual language is dark, layered and state-forward: a near-black frame around a floating content panel, low-chroma navy cards, crisp hairlines, compact controls, one action colour and a separate set of data colours.

Three sources make up the palette, each with one job:

- **Reef surfaces** come from the account-ready auth screen (`docs/public/images/auth-flow/account-ready.png`), darkened and desaturated. Its saturated navy card read as too bright behind dense data, so the surfaces keep its hue at about half the chroma.
- **Lilac** (`#bb9af7`, Tokyo Night's keyword colour) marks everything you can act on: buttons, links, checked controls, focus and the active nav icon.
- **Tokyo Night's data colours** mark everything you read as data: chart series, the activity heatmap and ranked meters.

The logo's ocean blue survives in the brand tile, the auth illustration and the glow behind the content panel.

The living reference is `/admin/design-system` (`crates/admin-ui/web/src/routes/design-system/`). It renders every token and installed component from the live theme, measures contrast in the browser and pins hover, focus, active and disabled states side by side. This file is the written source; the page is how you check it.

This system rejects decorative SaaS dashboards, glossy vanity-metric cards and consumer-friendly simplification that hides operational detail. The interface earns trust by showing real state clearly and keeping mutations scoped, legible and reversible where possible.

**Key Characteristics:**

- A floating content panel inside a darker frame, with cards one step above it.
- One action colour (lilac) and a separate set of data colours (Tokyo Night). Neither stands in for the other.
- Compact, familiar controls for experienced admins.
- Precise labels, visible state and quiet hierarchy.
- Restrained motion: one entrance per page, fades on state changes, fills that glide.

### Dark Only

The admin UI is dark only, on purpose. `<html>` always carries `.dark` so shadcn's `dark:` variants apply, every token lives on `:root` with `color-scheme: dark`, and there is no theme toggle or light palette. The shadcn design-system checklist expects a working theme toggle; this is a deliberate exception. Deriving a light theme is future work, not a gap to fill piecemeal. [ADR: Admin UI Design System, Dark Only](docs/adr/2026-10-16-admin-ui-design-system-dark-only.md) records the decision and its trade-offs.

## 2. Colors

Every value below is a token in `crates/admin-ui/web/src/styles/globals.css`. Contrast figures are measured on the design-system page.

### Surfaces, Darkest First

- **Frame** (`#040c15`, `--sidebar`): the sidebar and the margin around the content panel.
- **Content Panel** (`#07111c`, `--background`): the page itself, floating inside the frame.
- **Card** (`#0c1a29`, `--card`): cards and tables, one step above the panel.
- **Floating Layer** (`#0e2133`, `--popover`): popovers, menus, dialogs and sheets.
- **Secondary Fill** (`#102336`, `--secondary`) and **Muted Layer** (`#0c1f31`, `--muted`, `--sidebar-accent`): secondary controls, the active nav row and inactive tonal surfaces.
- **Hover Wash** (`#0d2841`, `--accent`): hover fills in menus and lists.
- **Table Header** (`#0c1e2f`, `--surface-muted`): table header rows.

### Text and Lines

- **Ink** (`#e8edf3`, `--foreground`): primary text. 16.1:1 on the panel, 14.9:1 on cards.
- **Soft Text** (`#94a7bb`, `--muted-foreground`): secondary labels and descriptions. 7.1:1 on cards. Do not push it lower for elegance.
- **Nav Text** (`#cad2db`, `--sidebar-foreground`): sidebar labels. 12.9:1 on the frame.
- **Hairline** (`rgb(236 240 247 / 0.08)`, `--border`; `/ 0.12` for `--input`): structure lines for cards, headers, tables and inputs.

### Actions

- **Lilac** (`#bb9af7`, `--primary`, `--ring`): primary buttons, links, checked controls, progress bars, focus rings and the active nav icon.
- **On Lilac** (`#040c15`, `--primary-foreground`): text on lilac fills. 8.5:1. White fails on lilac (2.3:1); never use it.
- Lilac as text on a card is 7.6:1, so `text-primary` and the `link` button variant are fine at body size.

### Brand Illustration

- **Logo Ocean Blue** (`#016f9f`, `--sidebar-primary`): the brand tile in the sidebar and the glow behind the content panel (`--panel-glow`).
- **Logo Mist** (`#b5dde5`, `--mist`), **Wave Foam** (`#d6f7f9`, `--wave-foam`) and **Wave Deep** (`#01294b`, `--wave-deep`): the auth hero illustration only. Never actions, never text.

### Status

- **Success Green** (`#73daca`, `--success`): success, active and healthy states. Tokyo Night's green1. 10.6:1 on cards and 5.9:1 as badge text on its own tint.
- **Warning** (`#eaa518`, `--warning`): warning state only. One token: `--color-warning` and the `text-warning` utility both resolve to it.
- **Destructive Rose** (`#fb7185`, `--destructive`): destructive state only. Rose-400 rather than red keeps it apart from every action colour. 6.5:1 on cards, 6.1:1 on popovers and 4.9:1 as text on its own tint.
- Success green is lighter than destructive rose, so "Active" and "Revoked" stay distinct for colour-blind readers, not just by hue.

### Data (Tokyo Night)

Charts read colours from tokens, never from the action colour.

- **Series** (`--chart-1` to `--chart-5`), always in this order: blue `#7aa2f7`, green `#9ece6a`, teal `#41a6b5`, cyan `#7dcfff`, purple `#9d7cd8`. Every series is at least 5.2:1 on cards.
- **Other** (`#737aa2`, `--chart-other`): the grouped series after the top five.
- **Hero** (`#7aa2f7`, `--chart-hero`): single-series charts (spend trend) and ranked meters (spend by model, spend by owner).
- **Heatmap** (`--heatmap-0` to `--heatmap-4`): `#172432` (empty), `#4d68a7`, `#6183d1`, `#7aa2f7`, `#89ddff`, from empty to busiest. Every active level is at least 3:1 on cards (3.2, 4.8, 7.0 and 11.6), so a day with a little activity never reads as empty; the empty level stays close to the card on purpose.

Two pairs sit close together, and both are accepted:

- Success green (`#73daca`) sits near the cyan and green series. Request logs show green `200` badges near charts, so keep status badges in tables and series in charts, and never use a chart colour for status.
- Blue and purple are close for colour-blind readers. Charts with crossing lines (leaderboard, agent harnesses) keep their legends and tooltips, so a series is never identified by colour alone.

### Named Rules

**The Action/Data Rule.** Lilac marks what you can act on; Tokyo Night colours mark data. Don't fill a button with a chart colour, and don't draw a series in lilac.

**The Accent Scarcity Rule.** Lilac identifies action, active state or focus. It is not decoration and should stay below roughly 10% of a screen.

**The Status Rule.** Green, amber and rose describe state. Never use them for selection, categories or decoration. Status badges always take a status variant: `success` for active and healthy, `warning` for pending, `destructive` for failed, and `secondary` for inactive. They never use the default (lilac) variant, which reads as an action.

**The Illustration-Only Rule.** Foam, mist, deep wave and the auth hero image belong to auth and error surfaces. Routine admin pages use surfaces, ink, lilac and data colours.

## 3. Typography

**Display Font:** Geist Variable, with `ui-sans-serif, sans-serif` fallback.
**Body Font:** Geist Variable, with `ui-sans-serif, sans-serif` fallback.
**Label/Mono Font:** Geist Variable for labels; use monospace only for identifiers, request IDs, keys, code and snippets.

**Character:** One technical sans family carries the full UI. The type should read as precise and operational, not branded for its own sake.

### Hierarchy

Each level is a Tailwind `@utility` in `globals.css`, so a role is one class: `type-display`, `type-headline`, `type-title`, `type-body`, `type-label`, `type-caption`. Page titles stay on `PageHeader`'s `text-2xl font-semibold tracking-tight`.

- **Display** (`type-display`, 500, `clamp(2.25rem, 2vw + 1.5rem, 3rem)`, `1.1`): reserved for auth and error surfaces, never routine card grids.
- **Headline** (`type-headline`, 500, `1rem`, `1.375` line-height): card titles and page-local section headings.
- **Title** (`type-title`, 500, `0.875rem`): table headers, dialog titles and compact panel headings.
- **Body** (`type-body`, 400, `clamp(15px, 0.28vw + 14px, 16px)`, `1.5` line-height): primary reading text and form descriptions.
- **Label** (`type-label`, 500, `0.875rem`): button text, field labels and nav items.
- **Caption** (`type-caption`, 500, `0.75rem`): badges, metadata and chart axes.

Letter spacing is 0 by default. Uppercase labels are tracked, and new ones take `tracking-label` (0.08em). The error page's eyebrow takes `tracking-eyebrow` (0.18em).

### Named Rules

**The One-Family Rule.** Do not introduce display fonts or decorative pairings in the admin UI. Product trust comes from consistency, not typographic novelty.

**The Identifier Rule.** Use monospace only when the string is machine-like: request IDs, keys, JSON, config snippets or code. Do not make the whole console terminal-themed.

## 4. Elevation

Five layers, darkest at the back. Depth comes from tonal steps first; shadows belong to the content panel and floating UI.

| Layer | Surface | Treatment | Token |
| --- | --- | --- | --- |
| Frame | `--sidebar` | Flat | – |
| Content panel | `--background` | Hairline ring, panel shadow, ocean glow at the top | `shadow-panel`, `--panel-glow` |
| Card | `--card` | Hairline ring, faint top highlight | `inset-shadow-highlight` |
| Popover | `--popover` | Hairline ring, popover shadow | `shadow-popover` |
| Dialog | `--popover` | Hairline ring, dialog shadow | `shadow-dialog` |

The values live in an `@theme` block in `globals.css`, so each is a Tailwind utility:

- `--shadow-panel`: `0 8px 24px -12px rgb(0 0 0 / 0.6)`
- `--inset-shadow-highlight`: `inset 0 1px 0 rgb(255 255 255 / 0.05)`
- `--shadow-popover`: `0 12px 28px -8px rgb(0 0 0 / 0.55)`
- `--shadow-dialog`: `0 24px 64px -16px rgb(0 0 0 / 0.7)`

### Named Rules

**The Border-First Rule.** If a surface can be understood with a tonal step and a hairline, do not add a shadow. Cards never cast one.

**The Floating-Only Rule.** Shadows mean "this floats": the content panel, menus, popovers, selects, dialogs and sheets. Nothing else.

## 5. Motion

Motion confirms what happened; it never decorates. Tokens live in `globals.css`.

| Token | Duration | Easing | Use |
| --- | --- | --- | --- |
| `--duration-fast` | 120ms | `ease-standard` | Hover and press colour changes, nav rows |
| `--duration-base` | 200ms | `ease-emphasized` | Page entrance, dialogs and overlays |
| `--duration-slow` | 320ms | `ease-emphasized` | Sheets, progress fills |
| `--duration-chart` | 600ms | ease-out | Chart series draw-in |

The easings are Tailwind utilities from `@theme`:

- `ease-standard`: `cubic-bezier(0.2, 0, 0, 1)`
- `ease-emphasized`: `cubic-bezier(0.22, 1, 0.36, 1)`
- `ease-exit`: `cubic-bezier(0.4, 0, 1, 1)`

Where motion appears:

- **Page entrance** (`animate-page-in`): the page content fades up 4px once when the route changes, and once on the auth screens.
- **Overlays:** menus and popovers fade and scale in at 100ms. Dialogs fade and scale in at `--duration-base`; sheets slide in at `--duration-slow`.
- **Navigation:** sidebar rows fade their colour changes.
- **Progress bars** glide to their new value.
- **Charts** draw in over 600ms, set once in `src/lib/chart-motion.ts`.

### Named Rules

**The One-Entrance Rule.** One entrance per view. No staggered table rows, no looping or ambient motion other than loading indicators.

**The Reduced-Motion Rule.** Everything stops under `prefers-reduced-motion`: a global rule in `globals.css` collapses CSS animations and transitions, and Recharts skips its draw-in on its own (`isAnimationActive` defaults to `'auto'`).

## 6. Components

### Buttons

- **Shape:** compact rounded rectangle (`rounded-lg`, derived from `--radius: 0.45rem`).
- **Primary:** lilac `bg-primary` with dark `text-primary-foreground`, height `2rem`, horizontal padding `0.625rem`, medium `0.875rem` text.
- **Hover / Focus:** hover dims the fill; focus uses a visible `ring-3` in lilac, not glow decoration.
- **Secondary / Outline / Ghost:** secondary uses the tonal fill, outline uses a border plus a dim fill, ghost uses the hover fill only.
- **Destructive:** a rose-tinted fill until confirmed.
- **Active:** non-menu buttons can translate down by `1px`; keep this tactile and subtle.

### Chips

- **Style:** badges are compact pills (`h-5`, `rounded-4xl`, `0.75rem` text) with variants for success, warning, destructive, outline and secondary.
- **State:** badge colour communicates state, not decoration. Status badges take a status variant, never the default lilac, and must stay readable at table density.

### Cards / Containers

- **Corner Style:** moderate rounded corners (`rounded-xl`), not oversized capsules.
- **Background:** `bg-card` with `text-card-foreground`, one step above the content panel.
- **Edges:** a `ring-1` hairline plus `inset-shadow-highlight`. No drop shadow.
- **Internal Padding:** default cards use `1rem`; small cards use `0.75rem`.

### Inputs / Fields

- **Style:** compact `h-8` controls with a transparent or dim background, a 1px input line and `rounded-lg` corners.
- **Focus:** `focus-visible:border-ring` and `focus-visible:ring-3` in lilac are required; focus must stay visible in dense forms.
- **Error / Disabled:** invalid fields shift to the destructive border and ring; disabled fields dim and block pointer events.

### Navigation

- **Shell:** the `inset` sidebar variant. The sidebar is the frame; the content panel floats inside it with a hairline, `shadow-panel` and the ocean glow.
- **Rows:** compact nav rows with Hugeicons at 16–18px. The active row takes `--sidebar-accent` and a lilac icon; colour changes fade over `--duration-fast`.
- **Brand tile:** the sidebar header tile keeps the logo's ocean blue (`--sidebar-primary`).
- **Typography:** nav labels are small, medium-weight and sentence case. Avoid tiny uppercase section eyebrows.

### Data Surfaces

Tables, virtualised lists, filters, sheets and detail panes are signature surfaces. Preserve density, make selected and detail state obvious, and keep filters close to the data they affect. Tables keep thin row lines; dense tables such as request logs and API keys need them to stay scannable. Empty states should teach the next operational step, not merely say that nothing exists.

**Numbers and time.** Each kind of value has one format, so a column compares at a glance:

- Numbers in tables are right-aligned and use `tabular-nums`.
- Counts group their digits (`9,812`). From 100,000 up they go compact (`34.4M`), with the exact count in a tooltip.
- Durations carry a unit and scale with size: `810 ms`, `3.96 s`, `1 h 6 min`. Never a bare `3964980ms`.
- Money keeps one precision per column: two decimals for totals, budgets and spend (`$34.40`, `formatUsd10000`), four for a single request's cost (`$0.0457`). A non-zero total too small for two decimals reads `<$0.01` (`-<$0.01` for a credit), never `$0.00` (`formatUsdTotal`).
- Timestamps read `16 Oct 2026, 02:04 UTC`: 24-hour, always with the zone, and `whitespace-nowrap` so they never wrap. Add seconds only where order within a minute matters, as in request logs.
- A missing value is an em dash (`—`), never `n/a`.

The helpers live in `src/lib/format.ts` (`formatCount`, `formatDuration`, `formatTimestamp`, `formatUsd10000`, `formatUsd10000Precise`, `EMPTY_VALUE`), and `CompactCount` renders a compact count with its tooltip. Use them rather than a local formatter.

**Wide tables.** A table never hides its last column without a cue. When a table can be wider than its card, pin its action column to the right edge with `sticky right-0`, the card's background and `shadow-sticky-edge-end`, the mirror of the `shadow-sticky-edge` that a column pinned left uses (as on the models table). If the action is the only one on the row, making the whole row open it works too. A table with no action column is sized to fit its card at 1440 px.

### Charts

- Series take `--chart-1` to `--chart-5` in order, then `--chart-other` for everything grouped.
- Single-series charts and ranked meters take `--chart-hero`.
- The activity heatmap takes `--heatmap-0` to `--heatmap-4`.
- Keep legends and tooltips on multi-series charts, so no series depends on colour alone.
- In a stack, neighbouring bands differ in lightness as well as hue. A stack whose series have fixed meanings alternates light and dark: Profile token volume runs Output `--chart-2`, Input `--chart-5`, Cached input `--chart-4`, bottom to top.
- Series and active heatmap levels are graphics, so they need 3:1 against the card (WCAG 1.4.11). The design-system page grades them, and `src/test/design-contrast.test.ts` fails the build when they, or a text pair under 4.5:1, slip.

## 7. Packaging and Reuse

The theme ships beyond the admin UI from the same tokens:

- **shadcn registry.** The root `registry.json` holds one `registry:theme` item, `oceans-theme`, generated from `globals.css` by `scripts/build-shadcn-theme.mjs`. Install it with `npx shadcn@latest add ahstn/oceans-llm/oceans-theme`, or from the built item at `https://docs.oceans-llm.com/r/oceans-theme.json`. It writes the colour tokens to `.dark` and radius and motion to `:root`. It mirrors the app's `@theme inline` block (colour mappings, the radius scale, the type and tracking extras, shadows and easings) and adds the `--color-*` aliases that components read with `var()`. It also adds the entrance keyframes, the `type-*` utilities and the reduced-motion rule. Run `mise run shadcn-theme` after changing tokens; `mise run lint` fails while `registry.json`, `docs/public/r` or the docs site's dark theme is out of date.
- **Docs site.** `docs/.vitepress/theme/custom.css` maps the same roles onto VitePress: Reef surfaces and lilac links in dark mode (the default), Tokyo Night Day equivalents in light mode, and Shiki's `tokyo-night` theme for dark code blocks.
- **Claude Design.** The "Oceans Gateway Admin" design system in Claude Design mirrors these tokens for mockups and decks. The repo stays the source of truth; re-sync it when tokens change.

## 8. Do's and Don'ts

### Do:

- **Do** use the existing token system in `src/styles/globals.css` before adding new values, and check new work against `/admin/design-system`.
- **Do** keep routine admin surfaces compact, legible and state-rich.
- **Do** use tonal layers and hairlines to organise dense data before reaching for shadows.
- **Do** reserve lilac for primary actions, links, active navigation, checked controls and focus.
- **Do** draw data in the Tokyo Night series, in order, with legends.
- **Do** keep focus states visible and test dense forms with keyboard navigation.
- **Do** check motion with `prefers-reduced-motion` turned on.

### Don't:

- **Don't** make decorative SaaS dashboards: no glossy cards, vanity metrics, busy gradients or ornamental chart chrome.
- **Don't** simplify away operational detail around permissions, budgets, request behaviour, lifecycle state or destructive actions.
- **Don't** use gradient text, side-stripe card accents, decorative grid backgrounds or glassmorphism as default styling.
- **Don't** introduce display fonts, oversized radii or theatrical page-load motion into the product UI.
- **Don't** put white text on lilac, use a chart colour for an action, or use lilac for data.
- **Don't** use green, amber or rose for anything but state.
