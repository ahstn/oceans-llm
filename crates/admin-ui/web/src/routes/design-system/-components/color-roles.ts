/**
 * The colour pairs the design-system page grades and src/test/design-contrast.test.ts enforces.
 * Text pairs need 4.5:1 (WCAG 1.4.3). Data pairs are graphics, so they need 3:1 (WCAG 1.4.11).
 * Names are CSS variables in globals.css.
 */
export const textRoles = [
  { name: 'Frame', background: 'sidebar', foreground: 'sidebar-foreground' },
  { name: 'Content panel', background: 'background', foreground: 'foreground' },
  { name: 'Card', background: 'card', foreground: 'card-foreground' },
  { name: 'Floating layer', background: 'popover', foreground: 'popover-foreground' },
  { name: 'Primary action', background: 'primary', foreground: 'primary-foreground' },
  { name: 'Secondary', background: 'secondary', foreground: 'secondary-foreground' },
  { name: 'Hover', background: 'accent', foreground: 'accent-foreground' },
  { name: 'Soft text on card', background: 'card', foreground: 'muted-foreground' },
  { name: 'Link text on card', background: 'card', foreground: 'primary' },
  { name: 'Focus ring on card', background: 'card', foreground: 'ring' },
  { name: 'Destructive on card', background: 'card', foreground: 'destructive' },
  { name: 'Success on card', background: 'card', foreground: 'success' },
] as const

export const dataRoles = [
  { name: 'Series 1 (blue)', background: 'card', foreground: 'chart-1' },
  { name: 'Series 2 (green)', background: 'card', foreground: 'chart-2' },
  { name: 'Series 3 (teal)', background: 'card', foreground: 'chart-3' },
  { name: 'Series 4 (cyan)', background: 'card', foreground: 'chart-4' },
  { name: 'Series 5 (purple)', background: 'card', foreground: 'chart-5' },
  { name: 'Other series', background: 'card', foreground: 'chart-other' },
  { name: 'Heatmap level 1', background: 'card', foreground: 'heatmap-1' },
  { name: 'Heatmap level 2', background: 'card', foreground: 'heatmap-2' },
  { name: 'Heatmap level 3', background: 'card', foreground: 'heatmap-3' },
  { name: 'Heatmap level 4', background: 'card', foreground: 'heatmap-4' },
] as const

export const TEXT_CONTRAST_MIN = 4.5
export const GRAPHIC_CONTRAST_MIN = 3
