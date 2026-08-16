# Agent Sessions — Detail View Design Proposals

Status: proposal (no live pages changed)
Source UI: `crates/admin-ui/web/src/routes/observability/agent-sessions.tsx` on branch `codex/analyze-codeburn-session-efficiency`
Stack: shadcn/ui (`radix-nova`, neutral base, hugeicons) + ReUI (`data-grid`, `filters`, `badge`)

Open `index.html` in a browser to browse the three mockups. Each mockup is a
self-contained static HTML file using the same data as the reference
screenshots (session `e9801b53…808b`, 27 Jul 2026, 22:02–22:31).

## Efficiency review of the current drawer

The drawer answers the wrong question first. For a monitoring surface the
operator's first question is "was this session healthy and efficient?" The
current layout answers "what are all the raw fields?" Inefficiencies found:

| # | Problem | Evidence in current UI |
|---|---------|------------------------|
| 1 | Headline answer requires scrolling and mental math | Active time 5.1 min vs elapsed 29.0 min vs excluded gaps 23.9 min are three separate rows in different sections. The 18% active ratio and the 9.8 s of real model work are never surfaced. |
| 2 | Duplicated data | "Requests (10)" and "Detected activity (10)" render the same 10 request IDs and timestamps twice — about 40% of the drawer length is redundant. |
| 3 | No visualization | 60+ label/value rows carry data that charts compress into one line: 6 coverage rows → 1 segmented bar, 8 token rows → 1 stacked bar, 8 time rows → 1 breakdown bar. |
| 4 | Uniform visual weight | "Analysis versions" (9 internal version strings) gets the same prominence as outcome and cost. "Succeeded" is plain text with no status color. |
| 5 | KPI cards scroll away | The 4 top metrics are gone after one viewport; the sheet has no sticky summary, anchors, or tabs. |
| 6 | Missing/empty states render at full weight | "—", "Not available", "0" rows (score, cache tokens, reasoning tokens) occupy the same space as real data. |
| 7 | Long IDs waste rows | Session ID / external session ID each take a full row, right-aligned, with no copy affordance. |

Reference patterns (research via Exa): Datadog APM trace view (header with
duration/status + flame/waterfall + tabbed metadata), LangSmith thread side
panel (summary first, drill-down second), Google SRE monitoring guidance
(every panel should answer a decision: triage, investigate, or ignore).

## The three proposals

### A — Triage Drawer (`design-a-tabbed-drawer.html`)

Evolution of the existing Sheet. Lowest implementation cost, keeps the
list → drawer navigation.

- Sticky header: outcome badge, score, and the 4 KPIs never scroll away.
- Efficiency strip: one segmented bar for 29.0 min elapsed (active 5.1 min,
  excluded gaps 23.9 min) with the 18% active ratio stated.
- Tabs replace the long scroll: **Overview** (identity as chips, coverage as
  one segmented bar, tokens as one stacked bar), **Activity** (requests and
  detected activity merged into one chronological ReUI Timeline), **Diagnostics**
  (score components, confidence, versions in Collapsibles).
- shadcn: `tabs` (new), `badge`, `card`, `collapsible`, `tooltip`. ReUI: `timeline`, `progress`.

### B — Session Report Page (`design-b-full-page.html`)

Promote the session to a full route (`/observability/agent-sessions/$sessionId`),
Linear/Vercel observability style. Best for deep investigation and sharing links.

- Header with breadcrumb, status, and action buttons.
- Stat card row with icons and sub-metrics (cost per request, tokens/s).
- Gantt-style timeline strip: 10 requests plotted on the 22:02–22:31 axis,
  the 23.9 min excluded-gap band rendered as a hatched region — the idle
  story is visible at a glance.
- Two-column body: main column (token mix stacked bar, merged activity feed),
  right rail (identity, score confidence, analysis versions in an accordion).
- shadcn: `card`, `chart` (already installed, recharts), `badge`, `breadcrumb`.
  ReUI: `timeline`, `data-grid` (activity export), `progress`.

### C — Ops Console (`design-c-compact-console.html`)

Dense, no-scroll glance view for monitoring many sessions. Monospace-forward,
status-light aesthetic.

- Radial gauge shows the active-time ratio (18%) — computable from existing
  data, honest while the session score is in calibration.
- Health chip row: outcome, confidence, coverage, cache savings as
  traffic-light chips.
- One merged activity feed with status dots and explicit gap markers
  ("6 min idle — excluded") instead of two tables.
- All diagnostics behind a single "Advanced" disclosure.
- shadcn: `badge`, `collapsible`, `scroll-area`. ReUI: `timeline` (compact variant).

## Recommendation

Ship **A** first (smallest diff to the current route, biggest readability
gain), then add **B** as the deep-link target the drawer's "Open full view"
action points to. **C** is the pattern to reuse later on the sessions list
page itself (per-row glance strip) rather than a third detail surface.

## Implementation notes

- `tabs` and `progress` are not yet in `src/components/ui` / ReUI — add via
  `npx shadcn@latest add tabs` and ReUI `progress` / `timeline`.
- Icon library is `hugeicons` (per `components.json`); mockups use inline SVG
  placeholders.
- Mockups use Tailwind CDN + neutral dark tokens to approximate the
  `radix-nova` dark theme; real implementation must use semantic tokens
  (`bg-background`, `text-muted-foreground`) per project shadcn rules.
