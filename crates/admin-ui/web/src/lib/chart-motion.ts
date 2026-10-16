/**
 * Series draw-in shared by every chart: --duration-chart (600ms), easing out. Recharts skips it when
 * the viewer prefers reduced motion, because isAnimationActive defaults to 'auto'. Kept free of
 * Recharts imports so lazily loaded charts can use it without pulling Recharts into a route chunk.
 */
export const chartAnimation = { animationDuration: 600, animationEasing: 'ease-out' } as const
