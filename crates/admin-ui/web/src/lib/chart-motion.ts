/**
 * Series draw-in shared by every chart: --duration-chart (600ms), easing out. Recharts needs a number,
 * so the value is repeated here; src/test/chart-motion.test.ts fails if it drifts from the token.
 *
 * The draw-in is JavaScript, so the CSS reduced-motion reset can't stop it. isAnimationActive 'auto'
 * is Recharts' own guard: it skips the animation when the viewer prefers reduced motion. It is
 * Recharts 3's default; setting it here keeps that behaviour if a future release changes the default.
 *
 * Kept free of Recharts imports so lazily loaded charts can use it without pulling Recharts into a
 * route chunk.
 */
export const chartAnimation = {
  animationDuration: 600,
  animationEasing: 'ease-out',
  isAnimationActive: 'auto',
} as const
