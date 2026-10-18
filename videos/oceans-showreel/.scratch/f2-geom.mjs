// Shared 3D layout for Frame 2 ("Flight"): plane placements, card rects, landing poses,
// and a projector that mirrors the CSS camera rig exactly.
//
// Rig (outer → inner, all 1920×1080 boxes with origin at the centre):
//   #f2-shake  translateY (deny jolts; a flat 2D jolt of the whole lens)
//   #f2-lens   perspective P (1000 except during the cmd-stop dolly zoom)
//   #f2-drift  translate3d(d) rotateX/Y (tiny, additive hold drift)
//   #f2-cam    translate3d(sx, sy, zs)
//   #f2-pitch  rotateX(rx)
//   #f2-yaw    rotateY(ry)
//   #f2-world  translate3d(-lx, -ly, -lz)
// A world point p (relative to the world centre) lands at q = S + Rx(rx)·Ry(ry)·(p − L),
// and on screen at 960 + qx·P/(P − qz), 540 + qy·P/(P − qz).

export const PERSP = 1000;
export const PW = 1600;
export const PH = 900;

const rad = (d) => (d * Math.PI) / 180;

export function rotY(a, [x, y, z]) {
  const c = Math.cos(rad(a));
  const s = Math.sin(rad(a));
  return [c * x + s * z, y, -s * x + c * z];
}
export function rotX(a, [x, y, z]) {
  const c = Math.cos(rad(a));
  const s = Math.sin(rad(a));
  return [x, c * y - s * z, s * y + c * z];
}
const add = (a, b) => a.map((v, i) => v + b[i]);
const sub = (a, b) => a.map((v, i) => v - b[i]);

// Story planes (five admin screens; the models and guardrails screens host two stops each) and
// dim filler planes for parallax depth.
export const PLANES = [
  { id: 'models', img: 'planes/models.jpg', pos: [0, 0, 0], a: 0 },
  { id: 'spend', img: 'planes/spend-controls.jpg', pos: [2300, -120, -250], a: -6 },
  { id: 'guard', img: 'planes/guardrails-settings.jpg', pos: [1250, 1750, -1300], a: 7 },
  { id: 'mcp', img: 'planes/mcp-invocations.jpg', pos: [2900, 2500, -2600], a: -5 },
  { id: 'skills', img: 'planes/skills.jpg', pos: [2800, 4400, -3100], a: -2 },
];
export const FILLERS = [
  { id: 'fa', img: 'wall/request-logs.jpg', pos: [-2500, -1000, -2100], a: 16 },
  { id: 'fb', img: 'wall/usage-costs.jpg', pos: [4500, 800, -2300], a: -18 },
  { id: 'fc', img: 'wall/agent-harnesses.jpg', pos: [-1800, 600, -3000], a: 14 },
  { id: 'fd', img: 'wall/leaderboard.jpg', pos: [5000, 3400, -4300], a: -14 },
];

// Seven stops. card = final rect of the lifted widget in plane px; src = the UI element it lifts
// out of (plane px); lift = translateZ of the card. The client stop is the routing card's back
// face (same centre line), so it has no lift of its own; its src is the row's Config button.
export const STOPS = [
  {
    key: 'route',
    plane: 0,
    card: { x: 520, y: 196, w: 920, h: 340 },
    src: { x: 296, y: 552, w: 1254, h: 59 },
    lift: 150,
    alpha: -6,
    beta: 5,
    anchor: [1140, 410],
  },
  {
    key: 'client',
    plane: 0,
    card: { x: 520, y: 96, w: 920, h: 540 },
    src: { x: 732, y: 568, w: 78, h: 27 },
    lift: 150,
    alpha: 5,
    beta: 3,
    anchor: [1100, 440],
  },
  {
    key: 'spend',
    plane: 1,
    card: { x: 560, y: 330, w: 860, h: 262 },
    src: { x: 296, y: 632, w: 1254, h: 64 },
    lift: 150,
    alpha: 5,
    beta: 3,
    anchor: [1140, 420],
  },
  {
    key: 'redact',
    plane: 2,
    card: { x: 470, y: 352, w: 900, h: 270 },
    src: { x: 718, y: 201, w: 412, h: 111 },
    lift: 150,
    alpha: -3,
    beta: 6,
    anchor: [1130, 420],
  },
  {
    key: 'cmd',
    plane: 2,
    card: { x: 20, y: 262, w: 1080, h: 432 },
    src: { x: 281, y: 201, w: 411, h: 111 },
    lift: 190,
    alpha: -2,
    beta: 2,
    anchor: [1110, 420],
  },
  {
    key: 'mcp',
    plane: 3,
    card: { x: 600, y: 270, w: 780, h: 350 },
    src: { x: 280, y: 200, w: 1286, h: 550 },
    lift: 150,
    alpha: 4,
    beta: 4,
    anchor: [1130, 450],
  },
  {
    key: 'skill',
    plane: 4,
    card: { x: 470, y: 300, w: 880, h: 380 },
    src: { x: 296, y: 372, w: 1254, h: 56 },
    lift: 150,
    alpha: 4,
    beta: 7,
    anchor: [1120, 420],
  },
];
export const stopIndex = (key) => STOPS.findIndex((st) => st.key === key);

export function planePoint(planeIdx, px, py, w = 0) {
  const P = PLANES[planeIdx];
  return add(P.pos, rotY(P.a, [px - PW / 2, py - PH / 2, w]));
}

export function landing(i, zs = 0) {
  const st = STOPS[i];
  const P = PLANES[st.plane];
  const L = planePoint(st.plane, st.card.x + st.card.w / 2, st.card.y + st.card.h / 2, st.lift);
  const k = PERSP / (PERSP - zs);
  return {
    lx: L[0],
    ly: L[1],
    lz: L[2],
    rx: st.beta,
    ry: st.alpha - P.a,
    sx: (st.anchor[0] - 960) / k,
    sy: (st.anchor[1] - 540) / k,
    zs,
  };
}

export function view(pose, p, drift = [0, 0, 0]) {
  const d = sub(p, [pose.lx, pose.ly, pose.lz]);
  const r = rotX(pose.rx, rotY(pose.ry, d));
  return add(add([pose.sx, pose.sy, pose.zs], r), drift);
}

export function project(q, P = PERSP) {
  const k = P / (P - q[2]);
  return [960 + q[0] * k, 540 + q[1] * k, k];
}

export const mixPose = (A, B, f) => {
  const o = {};
  for (const key of Object.keys(A)) o[key] = A[key] + (B[key] - A[key]) * f;
  return o;
};

// Deterministic soft "bokeh" dust in the flight volume (LCG, no Math.random).
export function dust(n = 64) {
  let s = 1234567;
  const rnd = () => {
    s = (s * 1103515245 + 12345) % 2147483648;
    return s / 2147483648;
  };
  const out = [];
  for (let i = 0; i < n; i++) {
    out.push({
      p: [-2600 + rnd() * 8200, -1800 + rnd() * 6200, -4800 + rnd() * 5200],
      size: 8 + Math.round(rnd() * 22),
      o: 0.12 + rnd() * 0.22,
      tint: rnd() < 0.3 ? 'wave' : 'mist',
    });
  }
  return out;
}
