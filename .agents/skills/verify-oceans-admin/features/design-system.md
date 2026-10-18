# Design System

The design system page is a living reference for the admin UI theme and components. It renders every token and installed shadcn component from the live theme, so it changes whenever `globals.css` or `src/components/ui/**` changes. `DESIGN.md` at the repo root is the written source.

## Sub-features

- `design-system-foundations` shows color pairs with a measured contrast ratio (`contrast-ratio` test ID), palette swatches (actions, brand illustration, surfaces, status, chart series and heatmap), the `type-*` scale, radius, spacing, the five-layer elevation ladder, and the motion tokens.
- `design-system-components` shows every component in `src/components/ui/`, with Button variants in a state matrix pinned through `data-preview`.
- `design-system-guidelines` shows the do and don't pairs and the recipes: auth hero panel, budget card, and the example list screen.

## How to get to it (user POV)

- Open `/admin/design-system` directly. It is not in the sidebar.
- Any signed-in user can open it; it is a personal path in `-auth-routing.ts`.

## Driving it with control-oceans-admin

Preconditions: a launched stack with the local demo seed, signed in as `alice@platform.local` / `localdemo123`.

- **Open.** Go to `/admin/design-system`. The heading reads `Design system`, the page starts at the top, and there are no console errors.
- **Sections.** The page has sections with the IDs `foundations`, `actions`, `inputs`, `data-display`, `navigation`, `feedback`, `overlays`, `guidelines`, and `recipes`.
- **Contrast.** There are 12 `contrast-ratio` badges and every one is at least `AA`. `Content panel` reads `16.13 AAA`, `Primary action` reads `8.50 AAA`, `Link text on card` reads `7.59 AAA`, `Destructive on card` reads `6.53 AA`, `Success on card` reads `10.55 AAA`, and the lowest, `Soft text on card`, reads `7.11 AAA`.
- **Data contrast.** The Data on cards block has 10 `graphic-contrast-ratio` badges (five series, Other and heatmap levels 1 to 4), and every one reads `Pass`, meaning at least 3:1 on the card. `Heatmap level 1` reads `3.22 Pass` and `Other series` reads `4.21 Pass`. `mise run //ui:test src/test/design-contrast.test.ts` checks the same pairs from `globals.css`.
- **Motion.** The Motion block lists `--duration-fast`, `--duration-base`, `--duration-slow` and `--duration-chart`, and `Replay` replays the page-entrance card and moves the fill bar.
- **Reduced motion.** With `prefers-reduced-motion: reduce` emulated, the Data display chart shows every bar at its final shape in its first frame and settles within a frame or two, once its legend is measured. Without the preference the bars draw in over about 600ms, across dozens of distinct frames.
- **Overlays.** `Open alert dialog` opens `Revoke ci-pipeline?` on the floating layer with the dialog shadow, and Escape closes it.
- **Responsive.** At 375px the document is no wider than the viewport.
- **Coverage.** `mise run //ui:test src/test/design-system-coverage.test.ts` passes. It fails when the page doesn't render a component in `src/components/ui/` (tests aside), or a part of one that the app renders: an imported name must appear as a JSX element, so a stale or type-only import doesn't count. A part also counts when a part the page renders renders it in turn, or when the admin shell renders it. CI runs it with the other admin UI tests.
- **Proof.** Capture the page, then `control-oceans-admin record design-system --check sections=pass --check contrast=pass --check data-contrast=pass --check reduced-motion=pass --check overlays=pass --check responsive=pass --check coverage=pass --artifact <screenshot>`.

## Gotchas

- The chart loads lazily; wait for the first `.recharts-surface` before capturing the Data display section.
- Ratios are measured from computed styles, so they change if a token changes. Update the expected values here when the palette moves.
- Page content fades up over 200ms after each navigation (`animate-page-in`). Wait about 300ms after the heading appears before capturing a screenshot.
- The inline Command is controlled so cmdk doesn't scroll its selected item into view on load. The first item still renders highlighted.
