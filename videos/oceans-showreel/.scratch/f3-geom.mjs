// Shared 3D layout for Frame 3 ("Lockup"): the 4×4 console wall, each tile's scattered start,
// the camera pull-back, and a projector that mirrors the CSS rig exactly.
//
// Rig (outer → inner, all 1920×1080 boxes with origin at the centre), one transform per wrapper:
//   #f3-cam   translate3d(sx, sy, zs)
//   #f3-pitch rotateX(rx)
//   #f3-yaw   rotateY(ry)
//   #f3-roll  rotate(rz)
//   .f3-tile  GSAP order: translate3d(x, y, z) rotate(tz) rotateY(ty) rotateX(tx), a W×H box
//             centred on (x, y), so a tile point is turned about X first, then Y, then Z.
// A wall point p lands at q = S + Rx·Ry·Rz·p, and on screen at 960 + qx·k, 540 + qy·k with
// k = 1000 / (1000 − qz).

export const PERSP = 1000;
export const W = 560;
export const H = 315;
export const GAP = 28;
export const DUR = 3.8;

const rad = (d) => (d * Math.PI) / 180;
export function rotX(a, [x, y, z]) {
  const c = Math.cos(rad(a));
  const s = Math.sin(rad(a));
  return [x, c * y - s * z, s * y + c * z];
}
export function rotY(a, [x, y, z]) {
  const c = Math.cos(rad(a));
  const s = Math.sin(rad(a));
  return [c * x + s * z, y, -s * x + c * z];
}
export function rotZ(a, [x, y, z]) {
  const c = Math.cos(rad(a));
  const s = Math.sin(rad(a));
  return [c * x - s * y, s * x + c * y, z];
}

export const EASE = {
  none: (p) => p,
  'power3.out': (p) => 1 - (1 - p) ** 4,
  'power4.out': (p) => 1 - (1 - p) ** 5,
  'expo.out': (p) => (p >= 1 ? 1 : 1 - 2 ** (-10 * p)),
};

// Wall: row-major 4×4. Story panels sit in the middle ring, where the wall is seen longest.
export const NAMES = [
  'agent-sessions', 'api-keys', 'teams', 'batches',
  'users', 'usage-costs', 'request-logs', 'model-routing-astra',
  'guardrails-settings', 'leaderboard', 'spend-increase', 'mcp-invocations',
  'agent-harnesses', 'models', 'request-inspect', 'spend-controls',
];

const GOLDEN = Math.PI * (3 - Math.sqrt(5));
// Depth rank per tile (a 4×4 ordered-dither pattern), so neighbours alternate near / far.
const DITHER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
const Z_NEAR = 300;
const Z_FAR = -620;
const SPREAD = 0.34; // extra radial distance as a fraction of the slot's own offset
export const TILES = NAMES.map((name, i) => {
  const c = i % 4;
  const r = Math.floor(i / 4);
  const x = (c - 1.5) * (W + GAP);
  const y = (r - 1.5) * (H + GAP);
  const a = i * GOLDEN;
  const ring = Math.max(Math.abs(c - 1.5), Math.abs(r - 1.5)) > 1 ? (Math.abs(c - 1.5) > 1 && Math.abs(r - 1.5) > 1 ? 2 : 1) : 0;
  const from = {
    x: Math.round(x * (1 + SPREAD) + Math.cos(a) * 70),
    y: Math.round(y * (1 + SPREAD) + Math.sin(a) * 50),
    z: Math.round(Z_NEAR - (DITHER[i] / 15) * (Z_NEAR - Z_FAR)),
    rotationX: Math.round(Math.sin(a) * 26),
    rotationY: Math.round(Math.cos(a) * 30),
    rotation: Math.round(Math.sin(2 * a) * 7),
  };
  return { i, name, c, r, ring, to: { x, y }, from };
});

// Tile flight: every tile moves from frame 0 (the cut is mid-motion); the centre ring lands first
// and the corners last, so arrival ripples outward without a parked, frozen start.
export const FLY = [1.0, 1.15, 1.3]; // duration by ring
export const FLY_EASE = 'power3.out';

// Camera: one long expo.out pull-back that settles through the hold, so the end card is never
// fully frozen until the last frames.
export const CAM_FROM = { sx: 0, sy: 0, zs: 150, rx: 8, ry: -4, rz: 2 };
export const CAM_TO = { sx: -150, sy: -80, zs: -620, rx: 22, ry: -15, rz: 6 };
export const CAM_EASE = 'expo.out';

export function camAt(t) {
  const e = EASE[CAM_EASE](Math.min(1, Math.max(0, t / DUR)));
  const o = {};
  for (const k of Object.keys(CAM_FROM)) o[k] = CAM_FROM[k] + (CAM_TO[k] - CAM_FROM[k]) * e;
  return o;
}

export function tileAt(T, t) {
  const p = Math.min(1, Math.max(0, t / FLY[T.ring]));
  const e = EASE[FLY_EASE](p);
  const f = T.from;
  const lerp = (a, b) => a + (b - a) * e;
  return {
    x: lerp(f.x, T.to.x),
    y: lerp(f.y, T.to.y),
    z: lerp(f.z, 0),
    rotationX: lerp(f.rotationX, 0),
    rotationY: lerp(f.rotationY, 0),
    rotation: lerp(f.rotation, 0),
  };
}

// Screen-space corners of a tile in pose `cam` with tile transform `tr`; also the nearest depth.
export function tileCorners(cam, tr) {
  const out = [];
  let zmax = -1e9;
  for (const [u, v] of [
    [-W / 2, -H / 2],
    [W / 2, -H / 2],
    [W / 2, H / 2],
    [-W / 2, H / 2],
  ]) {
    let p = rotX(tr.rotationX, [u, v, 0]);
    p = rotY(tr.rotationY, p);
    p = rotZ(tr.rotation, p);
    p = [p[0] + tr.x, p[1] + tr.y, p[2] + tr.z];
    p = rotZ(cam.rz, p);
    p = rotY(cam.ry, p);
    p = rotX(cam.rx, p);
    const q = [p[0] + cam.sx, p[1] + cam.sy, p[2] + cam.zs];
    zmax = Math.max(zmax, q[2]);
    const k = PERSP / (PERSP - q[2]);
    out.push([960 + q[0] * k, 540 + q[1] * k]);
  }
  return { pts: out, zmax };
}

if (process.argv[1] && process.argv[1].endsWith('f3-geom.mjs')) {
  const r0 = Math.round;
  for (const t of [0, 0.1, 0.35, 0.7, 1.0, 1.35, 2.0, 3.0, 3.8]) {
    const cam = camAt(t);
    let x0 = 1e9;
    let y0 = 1e9;
    let x1 = -1e9;
    let y1 = -1e9;
    let zmax = -1e9;
    let onScreen = 0;
    const near = [];
    for (const T of TILES) {
      const { pts, zmax: z } = tileCorners(cam, tileAt(T, t));
      zmax = Math.max(zmax, z);
      if (z > 700) near.push(`${T.i}:${r0(z)}`);
      const xs = pts.map((p) => p[0]);
      const ys = pts.map((p) => p[1]);
      x0 = Math.min(x0, ...xs);
      x1 = Math.max(x1, ...xs);
      y0 = Math.min(y0, ...ys);
      y1 = Math.max(y1, ...ys);
      if (Math.max(...xs) > 0 && Math.min(...xs) < 1920 && Math.max(...ys) > 0 && Math.min(...ys) < 1080) onScreen++;
    }
    console.log(
      `t=${t.toFixed(2)} cam zs ${r0(cam.zs)} rx ${cam.rx.toFixed(1)} ry ${cam.ry.toFixed(1)} rz ${cam.rz.toFixed(1)}  wall bbox (${r0(x0)},${r0(y0)})-(${r0(x1)},${r0(y1)})  on-screen ${onScreen}/16  zmax ${r0(zmax)}${near.length ? `  NEAR ${near.join(' ')}` : ''}`,
    );
  }
  if (process.env.TILES) {
    const t = Number(process.env.TILES);
    for (const T of TILES) {
      const { pts } = tileCorners(camAt(t), tileAt(T, t));
      console.log(`${String(T.i).padStart(2)} ${T.name.padEnd(20)} ${pts.map((p) => `(${r0(p[0])},${r0(p[1])})`).join(' ')}`);
    }
  }
}
