// Shared visibility math for Frame 2: near-clipped screen polygons, on-screen coverage,
// per-plane visibility spans (for culling) and the dust filter. The checker prints these;
// the builder turns the spans into visibility sets and keeps only the filtered dust.
import { PLANES, FILLERS, PW, PH, view, project, dust } from './f2-geom.mjs';
import { camAt, CUT } from './f2-path.mjs';

// Near clip sits 50px in front of the eye, which moves with the lens perspective P.
export const NEAR_GAP = 50;
export const FPS = 30;
export const ALL = [...PLANES, ...FILLERS];
export const HOLD_TIMES = [1.4, 3.4, 4.6, 6.4, 8.6, 10.6, 11.5, 12.6, 14.4, 16.4, 18.8];
const r0 = (v) => Math.round(v);

export function quad(P) {
  const c = Math.cos((P.a * Math.PI) / 180);
  const s = Math.sin((P.a * Math.PI) / 180);
  return [
    [0, 0],
    [PW, 0],
    [PW, PH],
    [0, PH],
  ].map(([px, py]) => {
    const u = px - PW / 2;
    const v = py - PH / 2;
    return [P.pos[0] + c * u, P.pos[1] + v, P.pos[2] - s * u];
  });
}

// Sutherland–Hodgman against an arbitrary half-space test.
export function clip(poly, inside, cross) {
  const out = [];
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i];
    const b = poly[(i + 1) % poly.length];
    const ia = inside(a);
    const ib = inside(b);
    if (ia) out.push(a);
    if (ia !== ib) out.push(cross(a, b));
  }
  return out;
}
export const lerp = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t);

export function screenPoly(pose, P) {
  const q = quad(P).map((c) => view(pose, c));
  const zn = pose.P - NEAR_GAP;
  const near = clip(
    q,
    (p) => p[2] <= zn,
    (a, b) => lerp(a, b, (zn - a[2]) / (b[2] - a[2])),
  );
  // kmax: the largest perspective scale any corner reaches (1 = on the view plane).
  const kmax = Math.max(...q.map((p) => (p[2] < pose.P - 1 ? pose.P / (pose.P - p[2]) : Infinity)));
  return { zmax: Math.max(...q.map((p) => p[2])), kmax, poly: near.map((p) => project(p, pose.P)) };
}

export function onScreen(poly, margin = 0) {
  let p = poly;
  const x0 = -margin;
  const x1 = 1920 + margin;
  const y0 = -margin;
  const y1 = 1080 + margin;
  p = clip(p, (v) => v[0] >= x0, (a, b) => lerp(a, b, (x0 - a[0]) / (b[0] - a[0])));
  p = clip(p, (v) => v[0] <= x1, (a, b) => lerp(a, b, (x1 - a[0]) / (b[0] - a[0])));
  p = clip(p, (v) => v[1] >= y0, (a, b) => lerp(a, b, (y0 - a[1]) / (b[1] - a[1])));
  p = clip(p, (v) => v[1] <= y1, (a, b) => lerp(a, b, (y1 - a[1]) / (b[1] - a[1])));
  if (p.length < 3) return { area: 0, bbox: null };
  let area = 0;
  for (let i = 0; i < p.length; i++) {
    const a = p[i];
    const b = p[(i + 1) % p.length];
    area += a[0] * b[1] - b[0] * a[1];
  }
  const xs = p.map((v) => v[0]);
  const ys = p.map((v) => v[1]);
  return {
    area: Math.abs(area) / 2,
    bbox: [r0(Math.min(...xs)), r0(Math.min(...ys)), r0(Math.max(...xs)), r0(Math.max(...ys))],
  };
}

// Frames where any part of the plane is within `margin` px of the frame, as [start, end) spans,
// plus the largest corner scale reached while shown and the times it exceeded 2.5×. `behind`
// lists frames where a corner is past the near plane: the plane must be hidden then unless it is
// on screen (`cross`, which the checker flags).
export function visibility(P, margin = 160) {
  const spans = [];
  const near = [];
  const behind = [];
  const cross = [];
  let open = null;
  let kmaxShown = 0;
  for (let f = 0; f <= Math.round(CUT * FPS); f++) {
    const t = f / FPS;
    const pose = camAt(t);
    const { zmax, kmax, poly } = screenPoly(pose, P);
    const vis = onScreen(poly, margin).area > 0;
    if (zmax >= pose.P - NEAR_GAP) {
      behind.push(t);
      if (onScreen(poly).area > 0) cross.push(t);
    }
    if (vis && open === null) open = t;
    if (!vis && open !== null) {
      spans.push([open, t]);
      open = null;
    }
    if (vis) {
      kmaxShown = Math.max(kmaxShown, kmax);
      if (kmax > 2.5) near.push(t);
    }
  }
  if (open !== null) spans.push([open, CUT]);
  return { spans, kmaxShown, near, behind, cross };
}

// Culling spans: pad each visible span by `pad` seconds, clamp to the frame, stop the padding
// short of any frame where a corner is past the near plane, and merge overlaps.
export function cullSpans(P, pad = 0.1) {
  const merged = [];
  const { spans, behind } = visibility(P);
  for (const [a, b] of spans) {
    let s = Math.max(0, a - pad);
    let e = Math.min(CUT, b + pad);
    for (const t of behind) {
      if (t < a && t >= s) s = t + 1 / FPS;
      if (t >= b && t < e) e = t;
    }
    const last = merged[merged.length - 1];
    if (last && s <= last[1]) last[1] = Math.max(last[1], e);
    else merged.push([s, e]);
  }
  return merged;
}

// Dust that never swells past 2.1× while near the frame and never sits on the card focus
// during a hold. Scale, not depth, is the budget: the dolly zoom shortens the eye distance.
export function keptDust(n = 110) {
  const kept = [];
  for (const d of dust(n)) {
    let kmax = 0;
    for (let f = 0; f <= Math.round(CUT * FPS); f++) {
      const pose = camAt(f / FPS);
      const q = view(pose, d.p);
      if (q[2] >= pose.P - NEAR_GAP) {
        kmax = Infinity;
        break;
      }
      const s = project(q, pose.P);
      if (s[0] > -200 && s[0] < 2120 && s[1] > -200 && s[1] < 1280) kmax = Math.max(kmax, s[2]);
    }
    let blocks = false;
    for (const t of HOLD_TIMES) {
      const pose = camAt(t);
      const s = project(view(pose, d.p), pose.P);
      if (s[0] > 600 && s[0] < 1700 && s[1] > 140 && s[1] < 720 && s[2] > 0.8) blocks = true;
    }
    if (kmax < 2.1 && !blocks) kept.push(d);
  }
  return kept;
}
