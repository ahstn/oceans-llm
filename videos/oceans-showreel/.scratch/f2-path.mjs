// Frame 2 camera schedule as data. The checker evaluates it (camAt) and the builder emits it
// as GSAP fromTo tweens on the rig wrappers, so the numbers checked are the numbers rendered.
import { PERSP, STOPS, landing, mixPose } from './f2-geom.mjs';

export const CUT = 20.0;
export const EASE = {
  none: (p) => p,
  'power2.in': (p) => p ** 3,
  'power3.in': (p) => p ** 4,
  'power2.out': (p) => 1 - (1 - p) ** 3,
  'power3.out': (p) => 1 - (1 - p) ** 4,
  'power4.out': (p) => 1 - (1 - p) ** 5,
  'power2.inOut': (p) => (p < 0.5 ? 4 * p ** 3 : 1 - (-2 * p + 2) ** 3 / 2),
  'power3.inOut': (p) => (p < 0.5 ? 8 * p ** 4 : 1 - (-2 * p + 2) ** 4 / 2),
  'sine.inOut': (p) => -(Math.cos(Math.PI * p) - 1) / 2,
};

export const L = STOPS.map((_, i) => landing(i));
export const START = { ...L[0], zs: -2400, rx: 30, ry: -26, sx: 0, sy: -60, P: PERSP };
const ORBIT_DIP = -220;
const DIP = -650;
const DOLLY_P = 440;
const CRANE_TILT = 9;
const EXIT_ZS = 420;
const KEYS = ['sx', 'sy', 'zs', 'rx', 'ry', 'lx', 'ly', 'lz'];
const pick = (P, keys) => Object.fromEntries(keys.map((k) => [k, P[k]]));
const without = (...drop) => KEYS.filter((k) => !drop.includes(k));

// Leg timings (local seconds). Each leg lands on the next stop; dolly runs during the cmd hold.
export const T = {
  dive0: [0, 0.7],
  orbit: [1.95, 0.8],
  whip: 5.1,
  swoop: [6.95, 0.7],
  pan: [9.35, 0.8],
  dolly: [10.0, 2.4],
  recoil: [12.85, 0.3],
  dive: [13.15, 0.55],
  crane: [17.2, 0.75],
  exit: [19.6, 0.4],
};

// Camera segments: { at, dur, ease, to: {key: value} }. Keys never overlap in time.
export const CAM = [];
const seg = (at, dur, ease, to) => CAM.push({ at, dur, ease, to });

seg(T.dive0[0], T.dive0[1], 'power4.out', pick(L[0], KEYS));
// Orbit: swing round the flipping card, easing back in depth to watch it turn over.
{
  const [at, dur] = T.orbit;
  seg(at, dur, 'power2.inOut', pick(L[1], without('zs')));
  seg(at, dur / 2, 'sine.inOut', { zs: ORBIT_DIP });
  seg(at + dur / 2, dur / 2, 'sine.inOut', { zs: L[1].zs });
}
// Whip (nudge curve): 10% wind-up, 75% burst, settle.
{
  const a = mixPose(L[1], L[2], 0.1);
  const b = mixPose(L[1], L[2], 0.75);
  seg(T.whip, 0.12, 'power3.in', pick(a, KEYS));
  seg(T.whip + 0.12, 0.1, 'none', pick(b, KEYS));
  seg(T.whip + 0.22, 0.35, 'power4.out', pick(L[2], KEYS));
}
// Swoop: inOut travel with a pull-back dip in depth at mid-leg.
{
  const [at, dur] = T.swoop;
  seg(at, dur, 'power2.inOut', pick(L[3], without('zs')));
  seg(at, dur / 2, 'sine.inOut', { zs: DIP });
  seg(at + dur / 2, dur / 2, 'sine.inOut', { zs: L[3].zs });
}
// Pan: a lateral glide across the guardrails page to the destructive-actions widget.
seg(T.pan[0], T.pan[1], 'power3.inOut', pick(L[4], KEYS));
// Dolly zoom: the perspective tightens while the card stays put (it sits on the view plane),
// so the page and the dust stretch away around it. It peaks on the last deny slam, then
// recoils to normal before the dive.
seg(T.dolly[0], T.dolly[1], 'sine.inOut', { P: DOLLY_P });
seg(T.recoil[0], T.recoil[1], 'power3.out', { P: PERSP });
seg(T.dive[0], T.dive[1], 'power4.out', pick(L[5], KEYS));
// Crane: drop down to the skills catalog, tilting over the top of the move.
{
  const [at, dur] = T.crane;
  seg(at, dur, 'power2.inOut', pick(L[6], without('rx')));
  seg(at, dur / 2, 'sine.inOut', { rx: (L[5].rx + L[6].rx) / 2 + CRANE_TILT });
  seg(at + dur / 2, dur / 2, 'sine.inOut', { rx: L[6].rx });
}
seg(T.exit[0], T.exit[1], 'power2.in', { zs: EXIT_ZS });

// Additive hold drift on the outer wrapper: d = translate(dx,dy,dz) rotateY(dry).
export const DRIFT = [];
const drift = (at, dur, to) => DRIFT.push({ at, dur, ease: 'sine.inOut', to });
drift(0.5, 1.4, { dx: -6, dy: 4, dz: 8, dry: 0.35 });
drift(2.8, 2.25, { dx: 5, dy: -3, dz: 0, dry: -0.3 });
drift(5.6, 1.35, { dx: -4, dy: 5, dz: 8, dry: 0.3 });
drift(7.6, 1.75, { dx: 6, dy: -4, dz: 0, dry: -0.25 });
drift(10.2, 2.9, { dx: -5, dy: 3, dz: 8, dry: 0.3 });
// MCP: a slow push-in across the long hold, so the denied chip reads while the frame stays alive.
drift(13.65, 3.55, { dx: -10, dy: 10, dz: 24, dry: 0.25 });
drift(18.0, 1.55, { dx: 1, dy: 2, dz: 16, dry: -0.3 });

// Resolve each segment's "from" values (the value of that key when the segment starts).
function resolve(list, init) {
  const state = { ...init };
  const sorted = list.slice().sort((x, y) => x.at - y.at);
  for (const s of sorted) {
    s.from = {};
    for (const k of Object.keys(s.to)) s.from[k] = state[k];
    for (const k of Object.keys(s.to)) state[k] = s.to[k];
  }
  return sorted;
}
export const CAM_R = resolve(CAM, START);
export const DRIFT0 = { dx: 0, dy: 0, dz: 0, dry: 0 };
export const DRIFT_R = resolve(DRIFT, DRIFT0);

function evalList(list, init, t) {
  const out = { ...init };
  for (const s of list) {
    if (t < s.at) continue;
    const p = Math.min(1, (t - s.at) / s.dur);
    const e = EASE[s.ease](p);
    for (const k of Object.keys(s.to)) out[k] = s.from[k] + (s.to[k] - s.from[k]) * e;
  }
  return out;
}

// Full camera (pose + drift) at local time t, in the form view() expects; drift is folded in
// approximately as a screen-space translation (rotation drift is < 0.5°).
export function camAt(t) {
  const p = evalList(CAM_R, START, t);
  const d = evalList(DRIFT_R, DRIFT0, t);
  return { ...p, sx: p.sx + d.dx, sy: p.sy + d.dy, zs: p.zs + d.dz, ry: p.ry + d.dry };
}
