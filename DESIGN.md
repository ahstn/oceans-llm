---
name: Oceans Gateway Admin
description: Same-origin operations console for the Oceans LLM Gateway.
colors:
  logo-ocean-blue: "#006f9f"
  logo-wave-blue: "#0b83ad"
  logo-mist-blue: "#b7dde4"
  success-green: "oklch(0.696 0.17 162.48)"
  background-dark: "oklch(0.145 0.012 238)"
  foreground-light: "oklch(0.985 0 0)"
  card-dark: "oklch(0.205 0 0)"
  muted-dark: "oklch(0.269 0 0)"
  muted-foreground-dark: "oklch(0.708 0 0)"
  border-dark: "oklch(1 0 0 / 10%)"
  warning: "oklch(0.769 0.156 77.727)"
  destructive: "oklch(0.704 0.191 22.216)"
typography:
  display:
    fontFamily: "Geist Variable, ui-sans-serif, sans-serif"
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
    backgroundColor: "{colors.logo-ocean-blue}"
    textColor: "{colors.foreground-light}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0 0.625rem"
  button-secondary:
    backgroundColor: "{colors.muted-dark}"
    textColor: "{colors.foreground-light}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0 0.625rem"
  card:
    backgroundColor: "{colors.card-dark}"
    textColor: "{colors.foreground-light}"
    rounded: "{rounded.xl}"
    padding: "1rem"
  input:
    backgroundColor: "transparent"
    textColor: "{colors.foreground-light}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0.25rem 0.625rem"
---

# Design System: Oceans Gateway Admin

## 1. Overview

**Creative North Star: "The Gateway Console"**

Oceans Gateway Admin is a focused, technical operations console that should feel native to the gateway runtime. Its visual language is dark, restrained, and state-forward: charcoal surfaces, crisp borders, compact controls, and a single accent role used to identify primary action and active state.

The system now uses the blues in `docs/public/images/oceans-logo-square.png` as the primary brand direction: deep ocean blue for primary action, brighter wave blue for emphasis and chart progression, and pale mist blue for logo-derived supporting surfaces. Green is reserved for semantic success, not brand identity.

This system rejects decorative SaaS dashboards, glossy vanity-metric cards, busy gradients, and consumer-friendly simplification that hides operational detail. The interface earns trust by showing real state clearly and keeping mutations scoped, legible, and reversible where possible.

**Key Characteristics:**

- Dark operations console with same-origin product feel.
- Tonal layering over decorative shadow.
- Compact, familiar controls for experienced admins.
- Precise labels, visible state, and quiet hierarchy.
- Logo-derived ocean blues are the intended primary color direction.

## 2. Colors

The palette is a charcoal console with a restrained operational accent: logo-derived ocean blue owns primary actions, active state, focus color, and the chart ramp.

### Primary

- **Logo Ocean Blue** (`#006f9f`): target primary action and selected-state color, derived from the darkest visible wave in the logo asset. Use where the current implementation uses `--primary` after the color replacement lands.
- **Logo Wave Blue** (`#0b83ad`): target secondary emphasis for charts, highlights, and active indicators that need separation from the primary action blue.
- **Logo Mist Blue** (`#b7dde4`): target pale supporting color for logo-adjacent assets and rare low-emphasis brand surfaces; do not use it as body text on dark surfaces.
- **Success Green** (`oklch(0.696 0.17 162.48)`): semantic success only. Do not use green for brand, navigation, or primary actions.

### Neutral

- **Console Background** (`oklch(0.145 0 0)`): dark page background used by the root document.
- **Panel Charcoal** (`oklch(0.205 0 0)`): card, popover, and sidebar surface in dark mode.
- **Muted Layer** (`oklch(0.269 0 0)`): secondary controls, hover fills, and inactive tonal surfaces.
- **Readable Ink** (`oklch(0.985 0 0)`): primary foreground on dark surfaces.
- **Soft Console Text** (`oklch(0.708 0 0)`): secondary labels and descriptions. Do not push it lighter or lower contrast for elegance.
- **Hairline Border** (`oklch(1 0 0 / 10%)`): default structure line for cards, headers, sidebars, and inputs.

### Tertiary

- **Warning Amber** (`oklch(0.769 0.156 77.727)`): warning state only.
- **Destructive Red** (`oklch(0.704 0.191 22.216)`): destructive state only.

### Named Rules

**The Accent Scarcity Rule.** The primary accent identifies action, active state, or meaningful status. It is not decoration and should stay below roughly 10% of a screen.

**The Logo Blue Rule.** Primary action, active navigation, focus color, and brand charts use the logo-blue family. Green is semantic success only.

## 3. Typography

**Display Font:** Geist Variable, with `ui-sans-serif, sans-serif` fallback.
**Body Font:** Geist Variable, with `ui-sans-serif, sans-serif` fallback.
**Label/Mono Font:** Geist Variable for labels; use monospace only for identifiers, request IDs, keys, code, and snippets.

**Character:** One technical sans family carries the full UI. The type should read as precise and operational, not branded for its own sake.

### Hierarchy

- **Display** (500, context-specific, tight but not cramped): reserved for auth and error surfaces, never routine card grids.
- **Headline** (500, `1rem`, `1.375` line-height): card titles and page-local section headings.
- **Title** (500, `0.875rem–1rem`): table headers, dialog titles, and compact panel headings.
- **Body** (400, `clamp(15px, 0.28vw + 14px, 16px)`, `1.5` line-height): primary reading text and form descriptions.
- **Label** (500, `0.75rem–0.875rem`): button text, field labels, nav items, badges, and metadata labels.

### Named Rules

**The One-Family Rule.** Do not introduce display fonts or decorative pairings in the admin UI. Product trust comes from consistency, not typographic novelty.

**The Identifier Rule.** Use monospace only when the string is machine-like: request IDs, keys, JSON, config snippets, or code. Do not make the whole console terminal-themed.

## 4. Elevation

Oceans Gateway Admin uses tonal layers and borders before shadows. Cards and panels are flat at rest, with 1px structure lines, dark surface steps, and restrained focus rings doing most of the depth work. Shadows exist as supporting tokens for panels, not as a general card style.

### Shadow Vocabulary

- **Panel Shadow** (`0 20px 45px color-mix(in oklch, black 86%, var(--primary) 14% / 0.24)`): reserved for elevated panels where separation from a dark background is required.
- **Soft Shadow** (`0 12px 28px color-mix(in oklch, black 88%, var(--primary) 12% / 0.14)`): rare ambient separation for important floating UI.

### Named Rules

**The Border-First Rule.** If a surface can be understood with a tonal layer and `1px` border, do not add a shadow.

**The No Ghost-Card Rule.** Do not pair decorative 1px borders with wide soft shadows on routine cards or buttons. Choose structure or elevation, not both.

## 5. Components

### Buttons

- **Shape:** compact rounded rectangle (`rounded-lg`, derived from `--radius: 0.45rem`).
- **Primary:** implementation uses logo-ocean `bg-primary text-primary-foreground`, height `2rem`, horizontal padding `0.625rem`, medium `0.875rem` text.
- **Hover / Focus:** hover adjusts fill opacity; focus uses visible `ring-3` and ring color, not glow decoration.
- **Secondary / Outline / Ghost:** secondary uses muted tonal fill, outline uses border plus transparent/dim fill, ghost uses hover fill only.
- **Active:** non-menu buttons can translate down by `1px`; keep this tactile and subtle.

### Chips

- **Style:** badges are compact pills (`h-5`, `rounded-4xl`, `0.75rem` text) with semantic variants for success, warning, destructive, outline, and secondary.
- **State:** use badge color to communicate state, not to decorate metadata. Status badges should remain readable at table density.

### Cards / Containers

- **Corner Style:** moderate rounded corners (`rounded-xl`), not oversized capsules.
- **Background:** `bg-card` on dark panel charcoal with `text-card-foreground`.
- **Shadow Strategy:** no default shadow; structure comes from `ring-1` or border plus tonal contrast.
- **Border:** rings and borders should be low-contrast but visible against the console background.
- **Internal Padding:** default cards use `1rem`; small cards use `0.75rem`.

### Inputs / Fields

- **Style:** compact `h-8` controls with transparent/dim background, 1px input border, and `rounded-lg` corners.
- **Focus:** `focus-visible:border-ring` and `focus-visible:ring-3` are required; focus must stay visible in dense forms.
- **Error / Disabled:** invalid fields shift to destructive border/ring; disabled fields dim and block pointer events.

### Navigation

- **Style:** inset sidebar with section groups, compact nav rows, Hugeicons at 18px, and active state handled by the shadcn sidebar vocabulary.
- **Typography:** nav labels are small, medium-weight, and sentence case. Avoid tiny uppercase section eyebrows.
- **Mobile treatment:** keep sidebar and breadcrumb behavior structural; do not rely on fluid type to solve responsive layout.

### Data Surfaces

Tables, virtualized lists, filters, sheets, and detail panes are signature surfaces. Preserve density, make selected/detail state obvious, and keep filters close to the data they affect. Empty states should teach the next operational step, not merely say that nothing exists.

## 6. Do's and Don'ts

### Do:

- **Do** use the existing token system in `src/styles/globals.css` before adding new values.
- **Do** keep routine admin surfaces compact, legible, and state-rich.
- **Do** use tonal layers and borders to organize dense data before reaching for shadows.
- **Do** reserve primary color for primary actions, active navigation, selected state, and meaningful status.
- **Do** keep logo-derived ocean blue as the primary action, focus, active-state, and chart-ramp family.
- **Do** keep focus states visible and test dense forms with keyboard navigation.

### Don't:

- **Don't** make decorative SaaS dashboards: no glossy cards, vanity metrics, busy gradients, or ornamental chart chrome.
- **Don't** simplify away operational detail around permissions, budgets, request behavior, lifecycle state, or destructive actions.
- **Don't** use gradient text, side-stripe card accents, decorative grid backgrounds, or glassmorphism as default styling.
- **Don't** introduce display fonts, oversized radii, or theatrical page-load motion into the product UI.
- **Don't** use green as brand color; reserve it for semantic success only.
