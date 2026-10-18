#!/usr/bin/env node
// Builds compositions/frames/f2-flight.html — Frame 2, "Flight".
// One continuous camera move across five admin screens laid out in 3D. At each of seven stops a
// UI element lifts off the page and morphs into a focused card that demonstrates the feature,
// while a ticker rail at the lower left names it. Geometry, the camera path and culling come from
// f2-geom / f2-path / f2-vis, so the numbers the checker verified are the numbers rendered.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { PLANES, FILLERS, PW, PH, STOPS, stopIndex, planePoint, view, project } from './f2-geom.mjs';
import { CAM_R, DRIFT_R, START, CUT, T, camAt } from './f2-path.mjs';
import { cullSpans, keptDust } from './f2-vis.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ICON_DIR = path.resolve(
  ROOT,
  '../../crates/admin-ui/web/node_modules/@lobehub/icons-static-svg/icons',
);
// Hugeicons (the admin UI's icon set), read from the per-icon ESM modules: [tag, attrs] lists.
const HUGE_DIR = path.resolve(
  ROOT,
  '../../crates/admin-ui/web/node_modules/@hugeicons/core-free-icons/dist/esm',
);
const HUGE = {};
for (const n of ['TaskDaily01Icon', 'Delete02Icon', 'CodeIcon', 'Copy01Icon', 'Tick02Icon', 'File01Icon', 'FileCodeIcon']) {
  HUGE[n] = (await import(pathToFileURL(path.join(HUGE_DIR, `${n}.js`)).href)).default;
}
const OUT = path.join(ROOT, 'compositions/frames/f2-flight.html');

const lobe = (name) =>
  fs
    .readFileSync(path.join(ICON_DIR, `${name}.svg`), 'utf8')
    .replace(/^[\s\S]*?<\/title>/, '')
    .replace(/<\/svg>\s*$/, '')
    .trim();
const icon = (name, cls) =>
  `<svg class="${cls}" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd" aria-hidden="true">${lobe(name)}</svg>`;
const SHIELD = `<svg class="f2-ico-18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M12 2.8l7.4 2.9v5.6c0 4.5-3 8.3-7.4 9.9-4.4-1.6-7.4-5.4-7.4-9.9V5.7z"/><path d="M8.9 12.1l2.2 2.2 4-4.3"/></svg>`;
const kebab = (k) => k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
const huge = (name, cls) =>
  `<svg class="${cls}" viewBox="0 0 24 24" fill="none" aria-hidden="true">${HUGE[name]
    .map(([tag, a]) => `<${tag} ${Object.entries(a).filter(([k]) => k !== 'key').map(([k, v]) => `${kebab(k)}="${v}"`).join(' ')}/>`)
    .join('')}</svg>`;

const r2 = (v) => Math.round(v * 100) / 100;
const r3 = (v) => Math.round(v * 1000) / 1000;
const px = (v) => `${r2(v)}px`;
const box = (l, t, w, h) =>
  `left:${px(l)};top:${px(t)}${w == null ? '' : `;width:${px(w)}`}${h == null ? '' : `;height:${px(h)}`}`;
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
// Rollers (tickers, tab copies, digit strips) roll through overflow-hidden masks, so all but the
// current item sit clipped outside the mask by design.
const MASKED = 'data-layout-allow-occlusion';
// The tab indicator carries a bright copy of the tab row, clipped to the pill, over the muted row.
const LAYERED = 'data-layout-allow-overlap';

// ---------------------------------------------------------------------------------------------
// Timeline ops. Collected here, sorted by time and emitted as literal GSAP calls. The first op
// in time on an element/property keeps fromTo's immediateRender; every later fromTo on that
// property gets immediateRender:false, so sequential segments never stomp each other at load.
const OPS = [];
let seq = 0;
const CTRL = new Set(['duration', 'ease', 'immediateRender', 'snap', 'stagger']);
function ft(sel, from, to, at) {
  OPS.push({ kind: 'fromTo', sel, from, to, at: r3(at), seq: seq++ });
}
function st(sel, vars, at) {
  if (at <= 0) throw new Error(`set at t<=0 on ${sel}; use inline CSS for initial state`);
  OPS.push({ kind: 'set', sel, vars, at: r3(at), seq: seq++ });
}
const propKeys = (vars) =>
  Object.keys(vars)
    .filter((k) => !CTRL.has(k))
    .flatMap((k) => (k === 'attr' ? Object.keys(vars.attr).map((a) => `attr.${a}`) : [k]));
// Only the earliest fromTo per element keeps immediateRender; every later one (any property)
// gets false, so a property first animated by a later fromTo must rest at its CSS value —
// emitOps reports any such property whose "from" disagrees with the declared baseline.
const BASELINE = { opacity: 1, x: 0, y: 0, z: 0, scale: 1, rotationX: 0, rotationY: 0, rotation: 0 };
const CSS_ZERO = /^#f2-(card-[a-z]+|chip-\d+)$/; // elements whose stylesheet opacity is 0
function emitOps() {
  const seenSel = new Set();
  const seenProp = new Set();
  const out = [];
  const sorted = OPS.slice().sort((a, b) => a.at - b.at || a.seq - b.seq);
  // Back-to-back segments on one property: at + duration can land a float ulp past the next
  // start (1.8 + 0.08 > 1.88), which reads as an overlap, so end those 0.1ms early instead.
  const starts = new Map();
  for (const op of sorted)
    for (const k of propKeys(op.kind === 'set' ? op.vars : op.to)) {
      const key = `${op.sel}|${k}`;
      if (!starts.has(key)) starts.set(key, []);
      starts.get(key).push(op.at);
    }
  const abuts = (op) =>
    propKeys(op.to).some((k) => {
      const end = op.at + (op.to.duration ?? 0);
      return starts.get(`${op.sel}|${k}`).some((t) => t > op.at && end > t && end - t < 1e-6);
    });
  for (const op of sorted) {
    const vars = op.kind === 'set' ? op.vars : op.to;
    const props = propKeys(vars);
    if (op.kind === 'fromTo') {
      const to = { ...op.to };
      if (abuts(op)) to.duration = Math.round((to.duration - 0.0001) * 1e4) / 1e4;
      if (!('immediateRender' in to) && seenSel.has(op.sel)) {
        to.immediateRender = false;
        for (const k of props) {
          if (seenProp.has(`${op.sel}|${k}`) || !(k in BASELINE)) continue;
          const rest = k === 'opacity' && CSS_ZERO.test(op.sel) ? 0 : BASELINE[k];
          if (op.from[k] !== rest) console.warn(`baseline mismatch: ${op.sel} ${k} from ${op.from[k]} rests at ${rest}`);
        }
      }
      out.push(`tl.fromTo(${JSON.stringify(op.sel)}, ${JSON.stringify(op.from)}, ${JSON.stringify(to)}, ${op.at});`);
      seenSel.add(op.sel);
    } else {
      out.push(`tl.set(${JSON.stringify(op.sel)}, ${JSON.stringify(op.vars)}, ${op.at});`);
    }
    props.forEach((k) => seenProp.add(`${op.sel}|${k}`));
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// Camera rig. Pose keys map onto wrapper transforms (world translation is negated).
const RIG = {
  sx: ['#f2-cam', 'x', 1],
  sy: ['#f2-cam', 'y', 1],
  zs: ['#f2-cam', 'z', 1],
  rx: ['#f2-pitch', 'rotationX', 1],
  ry: ['#f2-yaw', 'rotationY', 1],
  lx: ['#f2-world', 'x', -1],
  ly: ['#f2-world', 'y', -1],
  lz: ['#f2-world', 'z', -1],
  dx: ['#f2-drift', 'x', 1],
  dy: ['#f2-drift', 'y', 1],
  dz: ['#f2-drift', 'z', 1],
  dry: ['#f2-drift', 'rotationY', 1],
  // The CSS perspective property on the lens (not the transformPerspective transform function).
  P: ['#f2-lens', 'perspective', 1],
};
const rigVal = (prop, v) => (prop === 'perspective' ? `${r2(v)}px` : prop.startsWith('rotation') ? r3(v) : r2(v));
for (const s of [...CAM_R, ...DRIFT_R]) {
  const groups = {};
  for (const k of Object.keys(s.to)) {
    const [sel, prop, sign] = RIG[k];
    groups[sel] ||= { from: {}, to: {} };
    groups[sel].from[prop] = rigVal(prop, sign * s.from[k]);
    groups[sel].to[prop] = rigVal(prop, sign * s.to[k]);
  }
  for (const [sel, g] of Object.entries(groups)) {
    ft(sel, g.from, { ...g.to, duration: r3(s.dur), ease: s.ease }, s.at);
  }
}
const camInline = {
  cam: `translate3d(${px(START.sx)},${px(START.sy)},${px(START.zs)})`,
  pitch: `rotateX(${r3(START.rx)}deg)`,
  yaw: `rotateY(${r3(START.ry)}deg)`,
  world: `translate3d(${px(-START.lx)},${px(-START.ly)},${px(-START.lz)})`,
};

// Motion blur on the whole optical path: a seeded blur that clears as the dive lands, then one
// envelope per camera leg. The filter is switched off between legs so holds stay pin-sharp.
const MB = '#f2-mbg';
const sd = (v) => ({ attr: { stdDeviation: v } });
ft(MB, sd('20 20'), { ...sd('0 0'), duration: 0.6, ease: 'power3.out' }, 0);
st('#f2-optics', { filter: 'none' }, 0.6);
st('#f2-optics', { filter: 'url(#f2-mb)' }, T.whip);
ft(MB, sd('0 0'), { ...sd('22 0'), duration: 0.12, ease: 'power3.in' }, T.whip);
ft(MB, sd('22 0'), { ...sd('0 0'), duration: 0.35, ease: 'power4.out' }, T.whip + 0.22);
st('#f2-optics', { filter: 'none' }, T.whip + 0.57);
st('#f2-optics', { filter: 'url(#f2-mb)' }, T.swoop[0]);
ft(MB, sd('0 0'), { ...sd('4 14'), duration: 0.35, ease: 'sine.inOut' }, T.swoop[0]);
ft(MB, sd('4 14'), { ...sd('0 0'), duration: 0.35, ease: 'sine.inOut' }, T.swoop[0] + 0.35);
st('#f2-optics', { filter: 'none' }, T.swoop[0] + 0.7);
// Pan: a short lateral smear across the guardrails page.
st('#f2-optics', { filter: 'url(#f2-mb)' }, T.pan[0]);
ft(MB, sd('0 0'), { ...sd('8 3'), duration: 0.4, ease: 'sine.inOut' }, T.pan[0]);
ft(MB, sd('8 3'), { ...sd('0 0'), duration: 0.4, ease: 'sine.inOut' }, T.pan[0] + 0.4);
st('#f2-optics', { filter: 'none' }, T.pan[0] + 0.8);
st('#f2-optics', { filter: 'url(#f2-mb)' }, T.dive[0]);
ft(MB, sd('0 0'), { ...sd('14 14'), duration: 0.1, ease: 'power2.in' }, T.dive[0]);
ft(MB, sd('14 14'), { ...sd('0 0'), duration: 0.45, ease: 'power3.out' }, T.dive[0] + 0.1);
st('#f2-optics', { filter: 'none' }, T.dive[0] + 0.55);
// Crane: a vertical smear as the camera drops to the skills catalog.
st('#f2-optics', { filter: 'url(#f2-mb)' }, T.crane[0]);
ft(MB, sd('0 0'), { ...sd('4 16'), duration: 0.375, ease: 'sine.inOut' }, T.crane[0]);
ft(MB, sd('4 16'), { ...sd('0 0'), duration: 0.375, ease: 'sine.inOut' }, T.crane[0] + 0.375);
st('#f2-optics', { filter: 'none' }, T.crane[0] + 0.75);
st('#f2-optics', { filter: 'url(#f2-mb)' }, T.exit[0]);
ft(MB, sd('0 0'), { ...sd('24 24'), duration: 0.4, ease: 'power2.in' }, T.exit[0]);

// ---------------------------------------------------------------------------------------------
// Planes: culling and depth-of-field racks.
const ALLP = [...PLANES.map((P) => ({ ...P, story: true })), ...FILLERS];
const planeSel = (P) => `#f2-p-${P.id}`;
for (const P of ALLP) {
  const spans = cullSpans(P);
  spans.forEach(([a, b], i) => {
    if (a > 0 || i > 0) st(planeSel(P), { visibility: 'visible' }, a);
    if (b < CUT) st(planeSel(P), { visibility: 'hidden' }, b);
  });
}
const dofTo = (sel, from, to, at, dur, ease) =>
  ft(
    sel,
    { '--f2-dof': from[0], '--f2-dim': from[1] },
    { '--f2-dof': to[0], '--f2-dim': to[1], duration: dur, ease },
    at,
  );
// Each page racks sharp as the camera arrives, softens behind the lifted card, and blurs away on
// departure. The models page pulls back into focus for the Config click; the guardrails page
// sharpens for the pan to the second widget and falls deeper out of focus during the dolly zoom.
// [plane id, [[dof, dim], at, duration, ease]...] — each key chains from the previous one.
const DOF = [
  ['models', [[0, 1], 0, 0.48], [[3, 0.55], 0.48, 0.45], [[0.6, 0.85], 1.74, 0.14], [[3, 0.55], 2.02, 0.4, 'power2.inOut'], [[8, 0.45], T.whip, 0.5, 'power2.in']],
  ['spend', [[0, 1], T.whip + 0.1, 0.38], [[3, 0.55], T.whip + 0.48, 0.45], [[8, 0.45], T.swoop[0], 0.5, 'power2.in']],
  [
    'guard',
    [[0, 1], T.swoop[0] + 0.2, 0.43],
    [[3, 0.55], T.swoop[0] + 0.63, 0.45],
    [[1.5, 0.75], T.pan[0], 0.3, 'power2.inOut'],
    [[3, 0.55], T.pan[0] + 0.4, 0.25],
    [[4.5, 0.45], T.dolly[0], T.dolly[1], 'sine.inOut'],
    [[8, 0.4], T.dive[0], 0.4, 'power2.in'],
  ],
  ['mcp', [[0, 1], T.dive[0] + 0.03, 0.36], [[3, 0.55], T.dive[0] + 0.41, 0.45], [[8, 0.45], T.crane[0], 0.5, 'power2.in']],
  ['skills', [[0, 1], T.crane[0] + 0.2, 0.45], [[3, 0.55], T.crane[0] + 0.65, 0.45], [[8, 0.45], T.exit[0], 0.4, 'power2.in']],
];
for (const [id, ...keys] of DOF) {
  let from = [8, 0.5];
  for (const [to, at, dur, ease = 'power2.out'] of keys) {
    dofTo(`#f2-p-${id}`, from, to, at, dur, ease);
    from = to;
  }
}

// Contact shadow under a lifted card: a solid core plus a wide box-shadow falloff (cheaper to
// capture than filter: blur), offset down-right of the card like a key light from top-left.
const castBox = (card) => box(card.x + 38, card.y + 76, card.w - 76, card.h - 80);

// ---------------------------------------------------------------------------------------------
// The lift: highlight ring on the source row, a pixel-exact ghost of the row rises, and the card
// grows out of it (uniform scale + a clip band that opens to the full card while it lifts).
function liftHtml(st0, hc) {
  const { key, card, src } = st0;
  const P = PLANES[st0.plane];
  const s0 = src.w / card.w;
  const bh = Math.min(card.h, src.h / s0);
  let ct = Math.max(0, hc - bh / 2);
  if (ct + bh > card.h) ct = card.h - bh;
  const cb = card.h - ct - bh;
  const from = {
    x: r2(src.x - card.x),
    y: r2(src.y - card.y - ct * s0),
    z: 36,
    scale: r3(s0),
    clipPath: `inset(${px(ct)} 0px ${px(cb)} 0px round 16px)`,
  };
  return {
    from,
    surface: [
      `<div class="f2-cast" id="f2-cast-${key}" style="${castBox(card)}"></div>`,
      `<div class="f2-hl" id="f2-hl-${key}" style="${box(src.x - 6, src.y - 6, src.w + 12, src.h + 12)}"></div>`,
    ].join(''),
    ghost: `<div class="f2-ghost" id="f2-ghost-${key}" style="${box(src.x, src.y, src.w, src.h)};background-image:url(../../assets/${P.img});background-position:${px(-src.x)} ${px(-src.y)}"></div>`,
  };
}
function liftOps(st0, t0, from) {
  const k = st0.key;
  ft(`#f2-hl-${k}`, { opacity: 0, scale: 1.04 }, { opacity: 1, scale: 1, duration: 0.12, ease: 'power2.out' }, t0);
  ft(`#f2-hl-${k}`, { opacity: 1 }, { opacity: 0, duration: 0.2, ease: 'power1.in' }, t0 + 0.2);
  st(`#f2-ghost-${k}`, { opacity: 1 }, t0 + 0.07);
  ft(`#f2-ghost-${k}`, { z: 2, scale: 1 }, { z: 30, scale: 1.015, duration: 0.15, ease: 'power2.out' }, t0 + 0.07);
  ft(`#f2-ghost-${k}`, { opacity: 1 }, { opacity: 0, duration: 0.12, ease: 'none' }, t0 + 0.15);
  ft(`#f2-cast-${k}`, { opacity: 0 }, { opacity: 1, duration: 0.45, ease: 'power2.out' }, t0 + 0.13);
  // The card band fades up over the lifted ghost in place, then unfolds once it is opaque, so the
  // growing card never reads as a translucent sheet over the table.
  st(`#f2-card-${k}`, from, t0 + 0.08);
  ft(`#f2-card-${k}`, { opacity: 0 }, { opacity: 1, duration: 0.08, ease: 'power1.out' }, t0 + 0.1);
  ft(
    `#f2-card-${k}`,
    from,
    { x: 0, y: 0, z: st0.lift, scale: 1, clipPath: 'inset(0px 0px 0px 0px round 16px)', duration: 0.45, ease: 'power3.out' },
    t0 + 0.13,
  );
}
const departCard = (key, at) =>
  ft(`#f2-card-${key}`, { opacity: 1 }, { opacity: 0.4, duration: 0.4, ease: 'power2.in' }, at);

// ---------------------------------------------------------------------------------------------
// Stop 1 — model routing. One model, three provider routes, weighted-random selection.
const ROUTES = [
  { ico: 'aws-color', name: 'Bedrock', pill: ['Enabled', 'f2-pill-sec'], id: 'us.anthropic.claude-sonnet-4-6-v1:0', pw: 'Priority 10 · Weight 7', pct: 70 },
  { ico: 'vertexai-color', name: 'Vertex AI', pill: ['Enabled', 'f2-pill-sec'], id: 'anthropic/claude-sonnet-4-6', pw: 'Priority 10 · Weight 3', pct: 30 },
  { ico: 'openrouter', name: 'OpenRouter', pill: ['Fallback', 'f2-pill-out'], id: 'anthropic/claude-sonnet-4.6', pw: 'Priority 20 · Weight 1', pct: null },
];
const ROW0 = 98;
const ROWH = 72;
function card0Html() {
  const rows = ROUTES.map((r, i) => {
    const pct = r.pct == null ? '<span class="f2-mut">—</span>' : `<span id="f2-c1-pct-${i}">0</span>%`;
    return `
            <div class="f2-row" id="f2-c1-row-${i}" style="top:${ROW0 + i * ROWH}px">
              <div class="f2-row-in"${r.pct == null ? ' style="opacity:0.45"' : ''}>
                <div class="f2-ibox" style="${box(28, 14)}">${icon(r.ico, 'f2-ico-26')}</div>
                <div class="f2-abs f2-rname" style="${box(88, 10)}"><span>${r.name}</span><span class="f2-pill ${r.pill[1]}">${r.pill[0]}</span></div>
                <div class="f2-abs f2-mono f2-mut f2-rid" style="${box(88, 42)}">${r.id}</div>
                <div class="f2-abs f2-mut f2-pw" style="${box(600, 13)}">${r.pw}</div>
                <div class="f2-track" style="${box(600, 42, 200, 8)}"><div class="f2-fill" id="f2-c1-bar-${i}"></div></div>
                <div class="f2-abs f2-pct" style="${box(800, 19, 92)}">${pct}</div>
              </div>
            </div>`;
  }).join('');
  return `
          <div class="f2-card" id="f2-card-route" style="width:920px;height:340px">
            <div class="f2-ibox f2-ibox-plain" style="${box(26, 28)}">${icon('claude-color', 'f2-ico-34')}</div>
            <div class="f2-abs f2-mono f2-c1-model" style="${box(80, 24)}">claude-sonnet-4.6</div>
            <div class="f2-abs f2-c1-policy" style="right:28px;top:22px"><div class="f2-mut f2-lbl">Selection policy</div><div>Weighted random</div></div>
            <div class="f2-rule" style="top:90px"></div>
            <div class="f2-sel" id="f2-c1-sel" style="top:${ROW0 + 4}px"></div>
            <div class="f2-sel f2-ping" id="f2-c1-ping" style="top:${ROW0 + 4}px"></div>${rows}
          </div>`;
}
function stop0Ops() {
  ROUTES.forEach((r, i) => {
    const at = 0.6 + i * 0.06;
    st(`#f2-c1-row-${i}`, { opacity: 1 }, at);
    ft(`#f2-c1-row-${i}`, { y: 40 }, { y: 0, duration: 0.12, ease: 'power4.out' }, at);
    if (r.pct != null) {
      ft(`#f2-c1-bar-${i}`, { scaleX: 0 }, { scaleX: r.pct / 100, duration: 0.6, ease: 'power2.out' }, 0.85);
      ft(`#f2-c1-pct-${i}`, { innerText: 0 }, { innerText: r.pct, snap: { innerText: 1 }, duration: 0.6, ease: 'power2.out' }, 0.85);
    }
  });
  // Weighted-random picks: Bedrock, Vertex AI, Bedrock — then the pick confirms with a ping.
  ft('#f2-c1-sel', { opacity: 0, scale: 1.03 }, { opacity: 1, scale: 1, duration: 0.12, ease: 'power3.out' }, 1.12);
  ft('#f2-c1-sel', { y: 0 }, { y: ROWH, duration: 0.12, ease: 'power3.out' }, 1.3);
  ft('#f2-c1-sel', { y: ROWH }, { y: 0, duration: 0.12, ease: 'power3.out' }, 1.48);
  ft('#f2-c1-ping', { opacity: 0.8, scale: 1 }, { opacity: 0, scale: 1.035, duration: 0.4, ease: 'power2.out', immediateRender: false }, 1.62);
}

// ---------------------------------------------------------------------------------------------
// Stop 2 — client config. A cursor clicks the row's Config button and the routing card flips
// over to the generated client config. It pages through OpenCode, Pi and Claude Code with the
// gateway base URL lit in each, then copies the JSON.
const CURSOR_HTML = `
        <div class="f2-cursor" id="f2-cursor" style="${box(773, 582, 22, 30)}"><svg viewBox="0 0 22 30" aria-hidden="true"><path d="M3 2 L3 24 L8.5 18.8 L12.2 27 L15.6 25.5 L12 17.5 L19.5 17.5 Z"/></svg></div>`;
const CC_W = 864;
const CC_ROW0 = 56;
const CC_ROWH = 25;
const CC_TABS = [
  {
    ico: 'opencode',
    label: 'OpenCode',
    file: 'opencode.json',
    path: '~/.config/opencode/opencode.json',
    band: 9,
    rows: [
      [1, '{'],
      [2, '  "$schema": "https://opencode.ai/config.json",'],
      [3, '  "model": "oceans-llm/claude-sonnet-4.6",'],
      [4, '  "provider": {'],
      [5, '    "oceans-llm": {'],
      [null, '… 26 lines'],
      [32, '      "npm": "@ai-sdk/anthropic",'],
      [33, '      "options": {'],
      [34, '        "apiKey": "{env:OCEANS_LLM_API_KEY}",'],
      [35, '        "baseURL": "https://gateway.example.com"'],
    ],
  },
  {
    ico: 'pi',
    label: 'Pi',
    file: 'models.json',
    path: '~/.pi/agent/models.json',
    band: 5,
    rows: [
      [1, '{'],
      [2, '  "providers": {'],
      [3, '    "oceans-llm": {'],
      [4, '      "api": "anthropic-messages",'],
      [5, '      "apiKey": "$OCEANS_LLM_API_KEY",'],
      [6, '      "baseUrl": "https://gateway.example.com",'],
      [7, '      "compat": {'],
      [8, '        "forceAdaptiveThinking": true'],
      [9, '      },'],
      [10, '      "models": ['],
    ],
  },
  {
    ico: 'claudecode',
    label: 'Claude Code',
    file: 'settings.json',
    note: 'Gateway model settings',
    path: '~/.claude/settings.json',
    band: 4,
    rows: [
      [1, '{'],
      [2, '  "$schema": "https://json.schemastore.org/claude-code-settings.json",'],
      [3, '  "env": {'],
      [4, '    "ANTHROPIC_AUTH_TOKEN": "<gateway api token>",'],
      [5, '    "ANTHROPIC_BASE_URL": "https://gateway.example.com",'],
      [6, '    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6",'],
      [7, '    "ANTHROPIC_MODEL": "claude-sonnet-4.6",'],
      [8, '    "ANTHROPIC_SMALL_FAST_MODEL": "claude-sonnet-4.6",'],
      [9, '    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY": "1"'],
      [10, '  },'],
    ],
  },
];
const TAB_AT = [3.4, 4.2];
const BAND_AT = [2.95, 3.62, 4.42];
const COPY_AT = 4.75;
// JSON syntax colours: keys, string values, literals, punctuation. Whitespace passes through.
const JSON_TOKEN = /("(?:[^"\\]|\\.)*")(?=\s*:)|("(?:[^"\\]|\\.)*")|\b(true|false|null)\b|([{}[\],:])/g;
const jsonHtml = (line) =>
  line.replace(JSON_TOKEN, (m, key, str, lit) => {
    const cls = key ? 'f2-jk' : str ? 'f2-js' : lit ? 'f2-jl' : 'f2-jp';
    return `<span class="${cls}">${esc(m)}</span>`;
  });
function ccPageHtml(tab, i) {
  const rows = tab.rows
    .map(([n, text], r) => {
      const code = n == null ? `<span class="f2-cc-fold">${text}</span>` : jsonHtml(text);
      return `<div class="f2-cc-row" style="top:${CC_ROW0 + r * CC_ROWH}px"><span class="f2-cc-ln">${n ?? ''}</span><span class="f2-cc-code">${code}</span></div>`;
    })
    .join('');
  const note = tab.note ? `<span class="f2-mut">${tab.note}</span>` : '';
  return `
                <div class="f2-cc-page" style="left:${i * CC_W}px">
                  <div class="f2-cc-band" id="f2-cc-band-${i}" style="top:${CC_ROW0 + tab.band * CC_ROWH}px"></div>
                  <div class="f2-abs f2-cc-file">${huge('FileCodeIcon', 'f2-ico-16')}<span class="f2-mono">${tab.file}</span>${note}</div>
                  ${rows}
                </div>`;
}
function clientCardHtml() {
  const tabs = (attr = '') =>
    CC_TABS.map((t) => `<span class="f2-cc-tab">${icon(t.ico, 'f2-ico-18')}<span${attr}>${t.label}</span></span>`).join('');
  const paths = CC_TABS.map((t) => `<span ${MASKED}>${t.path}</span>`).join('');
  const copy = [
    `<div class="f2-cc-crow" ${MASKED}>${huge('Copy01Icon', 'f2-ico-16')}<span>Copy JSON</span></div>`,
    `<div class="f2-cc-crow" ${MASKED}>${huge('Tick02Icon', 'f2-ico-16 f2-okc')}<span>Copied</span></div>`,
  ].join('');
  return `
          <div class="f2-card f2-card-pop" id="f2-card-client" style="width:920px;height:540px">
            <div class="f2-ibox" style="${box(28, 26)}">${huge('CodeIcon', 'f2-ico-22')}</div>
            <div class="f2-abs f2-cc-title" style="${box(86, 22)}">Client config</div>
            <div class="f2-abs f2-mut f2-cc-desc" style="${box(86, 54)}">claude-sonnet-4.6 via AWS Bedrock</div>
            <div class="f2-cc-tabs" style="${box(28, 100, 458, 44)}">
              <div class="f2-cc-trow f2-mut">${tabs()}</div>
              <div class="f2-cc-ind" id="f2-cc-ind" ${MASKED}><div class="f2-cc-trow" id="f2-cc-indin" ${MASKED}>${tabs(` ${LAYERED}`)}</div></div>
            </div>
            <div class="f2-abs f2-cc-pathrow" style="${box(28, 160)}"><span class="f2-mut">Configuration</span><span class="f2-cc-pmask" ${MASKED}><span class="f2-cc-pstrip f2-mono" id="f2-cc-pstrip">${paths}</span></span></div>
            <div class="f2-cc-panel" style="${box(28, 200, CC_W, 312)}">
              <div class="f2-cc-reveal" id="f2-cc-reveal" style="clip-path:inset(0px 0px 268px 0px)">
                <div class="f2-cc-strip" id="f2-cc-strip">${CC_TABS.map(ccPageHtml).join('')}
                </div>
              </div>
              <div class="f2-cc-hrule"></div>
              <div class="f2-cc-scan" id="f2-cc-scan"></div>
              <div class="f2-cc-copy" id="f2-cc-copy"><div class="f2-cc-cmask" ${MASKED}><div id="f2-cc-cstrip">${copy}</div></div></div>
              <div class="f2-cc-cping" id="f2-cc-cping"></div>
            </div>
          </div>`;
}
function clientOps() {
  // The cursor glides in as the route pick confirms, hovers Config, presses it and lets go.
  ft('#f2-cursor', { opacity: 0, x: 90, y: 70, z: 20 }, { opacity: 1, x: 0, y: 0, duration: 0.22, ease: 'power3.out' }, 1.62);
  ft('#f2-cursor', { scale: 1 }, { scale: 0.85, duration: 0.05, ease: 'power2.in' }, 1.88);
  ft('#f2-cursor', { scale: 0.85 }, { scale: 1, duration: 0.1, ease: 'power2.out' }, 1.93);
  ft('#f2-cursor', { opacity: 1 }, { opacity: 0, duration: 0.15, ease: 'power1.in' }, 2.02);
  ft('#f2-hl-client', { opacity: 0, scale: 1.15 }, { opacity: 1, scale: 1, duration: 0.14, ease: 'power2.out' }, 1.76);
  ft('#f2-hl-client', { opacity: 1, scale: 1 }, { opacity: 0, scale: 1.3, duration: 0.35, ease: 'power2.out' }, 1.9);
  // The button itself (a pixel copy of it) rises off the page and clicks down.
  st('#f2-ghost-client', { opacity: 1 }, 1.8);
  ft('#f2-ghost-client', { z: 2, scale: 1 }, { z: 10, scale: 1.06, duration: 0.08, ease: 'power2.out' }, 1.8);
  ft('#f2-ghost-client', { scale: 1.06 }, { scale: 0.97, duration: 0.05, ease: 'power2.in' }, 1.88);
  ft('#f2-ghost-client', { scale: 0.97 }, { scale: 1, duration: 0.12, ease: 'back.out(2)' }, 1.93);
  ft('#f2-ghost-client', { opacity: 1 }, { opacity: 0, duration: 0.2, ease: 'power1.in' }, 2.05);
  // Flip: both faces share the routing card's centre line. The pair rises while it turns so the
  // taller back face clears the page at 90°.
  st('#f2-card-route', { transformOrigin: '50% 50%' }, 2.02);
  ft('#f2-card-route', { rotationX: 0 }, { rotationX: 180, duration: 0.62, ease: 'power2.inOut' }, 2.02);
  st('#f2-card-client', { opacity: 1 }, 2.02);
  ft('#f2-card-client', { rotationX: -180, z: 150 }, { rotationX: 0, duration: 0.62, ease: 'power2.inOut' }, 2.02);
  ft('#f2-flip', { z: 0 }, { z: 180, duration: 0.31, ease: 'power2.inOut' }, 2.02);
  ft('#f2-flip', { z: 180 }, { z: 0, duration: 0.31, ease: 'power2.inOut' }, 2.33);
  ft('#f2-cast-route', { opacity: 1 }, { opacity: 0, duration: 0.4, ease: 'power1.inOut' }, 2.1);
  ft('#f2-cast-client', { opacity: 0 }, { opacity: 1, duration: 0.4, ease: 'power1.inOut' }, 2.3);
  // The first config writes itself in behind a scan line.
  ft('#f2-cc-reveal', { clipPath: 'inset(0px 0px 268px 0px)' }, { clipPath: 'inset(0px 0px 0px 0px)', duration: 0.33, ease: 'power2.out' }, 2.62);
  st('#f2-cc-scan', { opacity: 1 }, 2.62);
  ft('#f2-cc-scan', { y: 0 }, { y: 268, duration: 0.33, ease: 'power2.out' }, 2.62);
  ft('#f2-cc-scan', { opacity: 1 }, { opacity: 0, duration: 0.15, ease: 'power1.in' }, 2.8);
  // Tabs: the indicator answers the click at once, carrying a counter-moving bright copy of the
  // labels inside it; the code pages across and the path rolls a beat behind.
  TAB_AT.forEach((at, k) => {
    ft('#f2-cc-ind', { x: 150 * k }, { x: 150 * (k + 1), duration: 0.28, ease: 'power3.out' }, at);
    ft('#f2-cc-indin', { x: -150 * k }, { x: -150 * (k + 1), duration: 0.28, ease: 'power3.out' }, at);
    ft('#f2-cc-strip', { x: -CC_W * k }, { x: -CC_W * (k + 1), duration: 0.42, ease: 'power3.inOut' }, at + 0.03);
    ft('#f2-cc-pstrip', { y: -24 * k }, { y: -24 * (k + 1), duration: 0.3, ease: 'back.out(1.4)' }, at + 0.06);
  });
  BAND_AT.forEach((at, i) =>
    ft(`#f2-cc-band-${i}`, { opacity: 0, scaleX: 0 }, { opacity: 1, scaleX: 1, duration: 0.3, ease: 'power3.out' }, at),
  );
  ft('#f2-cc-copy', { scale: 1 }, { scale: 0.94, duration: 0.05, ease: 'power2.in' }, COPY_AT - 0.05);
  ft('#f2-cc-copy', { scale: 0.94 }, { scale: 1, duration: 0.15, ease: 'back.out(2)' }, COPY_AT);
  ft('#f2-cc-cstrip', { y: 0 }, { y: -28, duration: 0.3, ease: 'back.out(1.4)' }, COPY_AT);
  ft('#f2-cc-cping', { opacity: 0.9, scale: 1 }, { opacity: 0, scale: 1.25, duration: 0.45, ease: 'power2.out', immediateRender: false }, COPY_AT + 0.02);
}

// ---------------------------------------------------------------------------------------------
// Stop 3 — spend controls. Grace's monthly budget fills to 88% and crosses into "Near limit".
// Counter and bar share one power2.out curve; the status flips exactly where the fill crosses
// 20% and 80% of the budget, and the count lands with ~0.35s of rest before the swoop.
const AMT_AT = 5.75;
const AMT_DUR = 0.85;
const AMT_FILL = 0.88;
const crossAt = (f) => r3(AMT_AT + AMT_DUR * (1 - Math.cbrt(1 - f / AMT_FILL)));
const ON_TRACK_AT = crossAt(0.2);
const NEAR_AT = crossAt(0.8);
function card1Html() {
  return `
          <div class="f2-card" id="f2-card-spend" style="width:860px;height:262px">
            <div class="f2-avatar" style="${box(28, 24)}">G</div>
            <div class="f2-abs f2-c2-name" style="${box(96, 24)}">Grace Applied Engineer</div>
            <div class="f2-abs f2-mut f2-c2-mail" style="${box(96, 53)}">grace@applied.local · Applied AI</div>
            <div class="f2-status" style="${box(700, 34)}">
              <div class="f2-status-bg"></div>
              <div class="f2-status-bg f2-status-warn" id="f2-c2-warnbg"></div>
              <div class="f2-status-mask"><div id="f2-c2-status"><div>Low usage</div><div>On track</div><div class="f2-warn-t">Near limit</div></div></div>
            </div>
            <div class="f2-status-ping" id="f2-c2-ping" style="${box(700, 34)}"></div>
            <div class="f2-abs f2-mut f2-c2-lbl" style="${box(28, 118)}">Current spend</div>
            <div class="f2-abs f2-c2-amtrow" style="right:28px;top:96px"><span class="f2-c2-amt">$<span id="f2-c2-amt">0</span>.00</span><span class="f2-mut f2-c2-of">of $150.00</span></div>
            <div class="f2-track f2-track-lg" style="${box(28, 170, 804, 10)}">
              <div class="f2-fillwrap" id="f2-c2-fill">
                <div class="f2-fill-l f2-fill-low"></div>
                <div class="f2-fill-l f2-fill-pri" id="f2-c2-fpri"></div>
                <div class="f2-fill-l f2-fill-warn" id="f2-c2-fwarn"></div>
              </div>
            </div>
            <div class="f2-tick" style="${box(28 + 804 * 0.8 - 1, 164)}"></div>
            <div class="f2-abs f2-mut f2-tick-l" style="${box(28 + 804 * 0.8 - 30, 190, 60)}">80%</div>
            <div class="f2-abs f2-pills" style="${box(28, 206)}"><span class="f2-pill f2-pill-out">Monthly</span><span class="f2-pill f2-pill-out">Config (user)</span></div>
          </div>`;
}
function stop1Ops() {
  ft('#f2-c2-amt', { innerText: 0 }, { innerText: 132, snap: { innerText: 1 }, duration: AMT_DUR, ease: 'power2.out' }, AMT_AT);
  ft('#f2-c2-fill', { scaleX: 0 }, { scaleX: AMT_FILL, duration: AMT_DUR, ease: 'power2.out' }, AMT_AT);
  st('#f2-c2-fpri', { opacity: 1 }, ON_TRACK_AT);
  st('#f2-c2-fwarn', { opacity: 1 }, NEAR_AT);
  ft('#f2-c2-status', { y: 0 }, { y: -32, duration: 0.39, ease: 'back.out(1.4)' }, ON_TRACK_AT);
  ft('#f2-c2-status', { y: -32 }, { y: -64, duration: 0.4, ease: 'back.out(1.4)' }, NEAR_AT);
  ft('#f2-c2-warnbg', { opacity: 0 }, { opacity: 1, duration: 0.15, ease: 'none' }, NEAR_AT);
  ft('#f2-c2-ping', { opacity: 0.9, scale: 1 }, { opacity: 0, scale: 1.25, duration: 0.45, ease: 'power2.out', immediateRender: false }, NEAR_AT);
}

// ---------------------------------------------------------------------------------------------
// Stop 4 — secret redaction. A prompt carries a (fake) Anthropic key; secret redaction flips it
// to the placeholder before it can reach a provider.
const KEY_OLD = 'sk-ant-api03-EXAMPLE-k3Yx9mQ';
const KEY_NEW = '[REDACTED:anthropic-api-key]';
if (KEY_OLD.length !== 28 || KEY_NEW.length !== 28) throw new Error('key strings must be 28 chars');
const GLYPHS = '#%*+=/\\|{}?!$@0123456789ABCDEFXYZ';
const pseudoGlyph = (seed) => GLYPHS[Math.floor((((seed * 9301 + 49297) % 233280) / 233280) * GLYPHS.length)];
const CELL = 16.8;
const KEY_X = 28 + 18 * CELL;
const FLIP_AT = 8.02;
const FLIP_STEP = 0.012;
function card2Html() {
  const cells = [...KEY_OLD]
    .map(
      (ch, c) =>
        `<span class="f2-cell" style="left:${px(KEY_X + c * CELL)}"><span class="f2-g f2-g-old" id="f2-k${c}o">${esc(ch)}</span><span class="f2-g f2-g-fl" id="f2-k${c}a">${esc(pseudoGlyph(c * 2 + 1))}</span><span class="f2-g f2-g-fl" id="f2-k${c}b">${esc(pseudoGlyph(c * 2 + 2))}</span><span class="f2-g f2-g-new" id="f2-k${c}n">${esc(KEY_NEW[c])}</span></span>`,
    )
    .join('');
  return `
          <div class="f2-card" id="f2-card-redact" style="width:900px;height:270px">
            <div class="f2-abs f2-c3-head" style="${box(28, 24)}"><span class="f2-pill f2-pill-sec">Prompt</span><span class="f2-mut">user message</span></div>
            <div class="f2-abs f2-mut f2-c3-tag" style="right:28px;top:26px">${SHIELD}<span>Guardrails</span></div>
            <div class="f2-abs f2-c3-msg" style="${box(28, 72)}">Why does the deploy fail? My .env:</div>
            <div class="f2-band f2-band-red" id="f2-c3-bred" style="${box(KEY_X - 7, 114, 28 * CELL + 14, 48)}"></div>
            <div class="f2-band f2-band-wave" id="f2-c3-bwave" style="${box(KEY_X - 7, 114, 28 * CELL + 14, 48)}"></div>
            <div class="f2-abs f2-mono f2-c3-env" style="${box(28, 116)}">ANTHROPIC_API_KEY=</div>
            <div class="f2-keyline" style="${box(0, 116)}">${cells}</div>
            <div class="f2-abs f2-pills" style="${box(28, 204)}"><span class="f2-pill f2-pill-warn" id="f2-c3-p0">Transformed</span><span class="f2-pill f2-pill-out f2-mono" id="f2-c3-p1">secret_redaction</span><span class="f2-pill f2-pill-out f2-mono" id="f2-c3-p2">provider_tokens</span></div>
          </div>`;
}
function stop2Ops() {
  for (let c = 0; c < 28; c++) {
    const s = FLIP_AT + c * FLIP_STEP;
    ft(`#f2-k${c}o`, { rotationX: 0 }, { rotationX: 90, duration: 0.1, ease: 'power2.in' }, s);
    st(`#f2-k${c}o`, { opacity: 0 }, s + 0.1);
    st(`#f2-k${c}a`, { opacity: 1 }, s + 0.1);
    st(`#f2-k${c}a`, { opacity: 0 }, s + 0.16);
    st(`#f2-k${c}b`, { opacity: 1 }, s + 0.16);
    st(`#f2-k${c}b`, { opacity: 0 }, s + 0.22);
    st(`#f2-k${c}n`, { opacity: 1 }, s + 0.22);
    ft(`#f2-k${c}n`, { rotationX: -90 }, { rotationX: 0, duration: 0.12, ease: 'power3.out' }, s + 0.22);
  }
  ft('#f2-c3-bred', { opacity: 1 }, { opacity: 0, duration: 0.45, ease: 'power1.inOut' }, FLIP_AT + 0.04);
  ft('#f2-c3-bwave', { opacity: 0 }, { opacity: 1, duration: 0.45, ease: 'power1.inOut' }, FLIP_AT + 0.04);
  [0, 1, 2].forEach((p) =>
    ft(`#f2-c3-p${p}`, { opacity: 0, y: 14 }, { opacity: 1, y: 0, duration: 0.4, ease: 'power3.out' }, FLIP_AT + 0.58 + p * 0.06),
  );
}

// ---------------------------------------------------------------------------------------------
// Stop 5 — destructive actions. The pack switches from Audit to Deny, then an agent types three
// shell commands and each one is denied the instant it lands. Every slam jolts the lens a little
// harder while the dolly zoom closes in. The reason codes are the ones the Snowflake, 1Password
// and AWS packs report for these exact commands.
const CMD_CH = 14.4; // JetBrains Mono 24px advance
const CMD_ROW0 = 128;
const CMD_ROWH = 74;
const CMDS = [
  { cmd: 'snow sql -q "DROP DATABASE analytics"', reason: 'snowflake.drop_database', pack: 'database.snowflake', at: 10.3, amp: 5 },
  { cmd: 'op vault delete Production', reason: 'onepassword.vault_delete', pack: 'secrets.onepassword', at: 11.1, amp: 7 },
  { cmd: 'aws s3 rb s3://prod-backups --force', reason: 'aws.destructive_operation', pack: 'cloud.aws', at: 11.9, amp: 10 },
].map((c) => ({ ...c, slam: r3(c.at + 0.5) }));
const MODE_AT = 10.12;
function cmdCardHtml() {
  const rows = CMDS.map((c, i) => {
    const w = r2(c.cmd.length * CMD_CH);
    return `
            <div class="f2-cm-row" id="f2-cm-row-${i}" style="top:${CMD_ROW0 + i * CMD_ROWH}px">
              <div class="f2-cm-rflash" id="f2-cm-rflash-${i}"></div>
              <div class="f2-abs f2-mono f2-mut f2-cm-line" style="left:34px">$</div>
              <div class="f2-cm-type" id="f2-cm-type-${i}" ${MASKED}><span class="f2-mono f2-cm-line f2-cm-cmd" id="f2-cm-cmd-${i}">${esc(c.cmd)}</span></div>
              <div class="f2-cm-caret" id="f2-cm-caret-${i}"></div>
              <div class="f2-cm-strike" id="f2-cm-strike-${i}" style="width:${px(w + 10)}"></div>
              <div class="f2-cm-tags"><span class="f2-pill f2-pill-out f2-mono" id="f2-cm-pack-${i}">${c.pack}</span><span class="f2-pill f2-pill-deny" id="f2-cm-deny-${i}">Denied</span></div>
            </div>`;
  }).join('');
  const modes = (cls) => `<span${cls[0]}>Audit</span><span${cls[1]}>Deny</span>`;
  const reasons = CMDS.map((c) => `<span ${MASKED}>${c.reason}</span>`).join('');
  return `
          <div class="f2-card" id="f2-card-cmd" style="width:1080px;height:432px">
            <div class="f2-ibox" style="${box(34, 31)}">${huge('Delete02Icon', 'f2-ico-22')}</div>
            <div class="f2-abs f2-cm-title" style="${box(103, 26)}">Destructive actions</div>
            <div class="f2-abs f2-mut f2-cm-desc" style="${box(103, 65)}">Packs that catch deletes, overwrites, and access removal.</div>
            <div class="f2-abs f2-mut f2-lbl" style="right:240px;top:48px">Mode</div>
            <div class="f2-cm-tog">
              <div class="f2-cm-slots f2-mut">${modes(['', ''])}</div>
              <div class="f2-cm-ind" id="f2-cm-ind" ${MASKED}>
                <div class="f2-cm-indbg"></div>
                <div class="f2-cm-indbg f2-cm-indred" id="f2-cm-indred"></div>
                <div class="f2-cm-slots" id="f2-cm-indin" ${MASKED}>${modes([' class="f2-fgc"', ' class="f2-destrc"'])}</div>
              </div>
            </div>
            <div class="f2-rule" style="top:108px"></div>${rows}
            <div class="f2-rule" style="top:358px"></div>
            <div class="f2-abs f2-cm-foot" id="f2-cm-foot" style="${box(34, 377)}"><span class="f2-destrc f2-cm-shield">${SHIELD}</span><span>Denied before execution</span><span class="f2-mut">·</span><span class="f2-cm-rmask" ${MASKED}><span class="f2-cm-rstrip f2-mono f2-mut" id="f2-cm-rstrip">${reasons}</span></span></div>
            <div class="f2-c4-flash" id="f2-cm-flash"></div>
          </div>`;
}
function cmdOps() {
  // Audit → Deny: the indicator slides across, carrying a bright copy of the labels, and reddens.
  ft('#f2-cm-ind', { x: 0 }, { x: 91, duration: 0.3, ease: 'power3.inOut' }, MODE_AT);
  ft('#f2-cm-indin', { x: 0 }, { x: -91, duration: 0.3, ease: 'power3.inOut' }, MODE_AT);
  ft('#f2-cm-indred', { opacity: 0 }, { opacity: 1, duration: 0.3, ease: 'power1.inOut' }, MODE_AT);
  CMDS.forEach((c, i) => {
    const n = c.cmd.length;
    const S = c.slam;
    ft(`#f2-cm-row-${i}`, { opacity: 0, y: 10 }, { opacity: 1, y: 0, duration: 0.12, ease: 'power2.out' }, c.at - 0.05);
    st(`#f2-cm-caret-${i}`, { opacity: 1 }, c.at - 0.05);
    ft(`#f2-cm-type-${i}`, { width: 0 }, { width: r2(n * CMD_CH), duration: 0.42, ease: `steps(${n})` }, c.at);
    ft(`#f2-cm-caret-${i}`, { x: 0 }, { x: r2(n * CMD_CH), duration: 0.42, ease: `steps(${n})` }, c.at);
    st(`#f2-cm-caret-${i}`, { opacity: 0 }, S);
    // The slam: Denied stamps down, the row flushes red, and the command is struck through.
    ft(`#f2-cm-deny-${i}`, { opacity: 0, scale: 1.6 }, { opacity: 1, scale: 1, duration: 0.3, ease: 'back.out(2)' }, S);
    ft(`#f2-cm-pack-${i}`, { opacity: 0, x: 12 }, { opacity: 1, x: 0, duration: 0.3, ease: 'power3.out' }, S + 0.06);
    ft(`#f2-cm-rflash-${i}`, { opacity: 0 }, { opacity: 1, duration: 0.05, ease: 'none' }, S);
    ft(`#f2-cm-rflash-${i}`, { opacity: 1 }, { opacity: 0.35, duration: 0.4, ease: 'power2.out' }, S + 0.05);
    ft(`#f2-cm-strike-${i}`, { scaleX: 0 }, { scaleX: 1, duration: 0.18, ease: 'power3.out' }, S + 0.02);
    ft(`#f2-cm-cmd-${i}`, { opacity: 1 }, { opacity: 0.45, duration: 0.2, ease: 'power1.out' }, S + 0.02);
    ft('#f2-cm-flash', { opacity: 0 }, { opacity: 1, duration: 0.05, ease: 'none' }, S);
    ft('#f2-cm-flash', { opacity: 1 }, { opacity: 0, duration: 0.33, ease: 'power2.out' }, S + 0.05);
    // A flat jolt of the whole lens, harder each time.
    const A = c.amp;
    ft('#f2-shake', { y: 0 }, { y: A, duration: 0.04, ease: 'power2.out' }, S);
    ft('#f2-shake', { y: A }, { y: -A / 2, duration: 0.06, ease: 'sine.inOut' }, S + 0.04);
    ft('#f2-shake', { y: -A / 2 }, { y: 0, duration: 0.1, ease: 'power2.out' }, S + 0.1);
  });
  ft('#f2-cm-foot', { opacity: 0, y: 10 }, { opacity: 1, y: 0, duration: 0.25, ease: 'power2.out' }, CMDS[0].slam);
  CMDS.slice(1).forEach((c, k) =>
    ft('#f2-cm-rstrip', { y: -29 * k }, { y: -29 * (k + 1), duration: 0.3, ease: 'back.out(1.4)' }, c.slam),
  );
}

// ---------------------------------------------------------------------------------------------
// Stop 6 — MCP tool sets. Tool chips hang in depth around the card; four snap into the set,
// the rest sink back, and github.delete_repository tries to dock and is denied.
const CHIP_CH = 10.8; // JetBrains Mono 18px advance
const CHIPS = [
  ['github', 'search_code', 'keep'],
  ['sentry', 'get_issue', 'keep'],
  ['github', 'create_issue', 'keep'],
  ['linear', 'create_issue', 'keep'],
  ['github', 'delete_repository', 'deny'],
  ['slack', 'post_message', 'drop'],
  ['github', 'merge_pull_request', 'drop'],
  ['notion', 'search', 'drop'],
  ['jira', 'transition_issue', 'drop'],
  ['linear', 'update_issue', 'drop'],
  ['sentry', 'resolve_issue', 'drop'],
  ['postgres', 'query', 'drop'],
].map(([srv, tool, role], i) => ({ srv, tool, role, i, w: r2((srv.length + 1 + tool.length) * CHIP_CH + 32) }));
const SLOTS = [
  [28, 110],
  [398, 110],
  [28, 184],
  [398, 184],
];
const MCP = stopIndex('mcp');
const CARD4 = STOPS[MCP].card;
// The cloud is art-directed in screen space at the landed MCP-stop camera — a ring around the
// card's on-screen rect (742,266)-(1517,623), clear of the HUD rail — then back-projected onto
// each chip's depth so the parallax during the dive is real. [centre x, centre y, z above page]
const CLOUD = [
  [620, 215, 420],
  [1700, 540, 360],
  [600, 680, 400],
  [1530, 190, 380],
  [1660, 370, 300],
  [330, 400, 240],
  [1200, 770, 420],
  [1010, 145, 160],
  [340, 580, 200],
  [1750, 720, 320],
  [1500, 860, 260],
  [880, 810, 120],
];
const CLOUD_T = r3(T.dive[0] + 0.5);
// Plane-local (px, py) whose projection at depth w lands on screen (X, Y): the view transform is
// affine in (px, py), so the perspective divide leaves two linear equations.
function unproject(t, X, Y, w) {
  const pose = camAt(t);
  const P = pose.P;
  const plane = STOPS[MCP].plane;
  const q0 = view(pose, planePoint(plane, 0, 0, w));
  const a = view(pose, planePoint(plane, 1, 0, w)).map((v, i) => v - q0[i]);
  const b = view(pose, planePoint(plane, 0, 1, w)).map((v, i) => v - q0[i]);
  const u = X - 960;
  const v = Y - 540;
  const a11 = -u * a[2] - P * a[0];
  const a12 = -u * b[2] - P * b[0];
  const b1 = -(u * (P - q0[2]) - P * q0[0]);
  const a21 = -v * a[2] - P * a[1];
  const a22 = -v * b[2] - P * b[1];
  const b2 = -(v * (P - q0[2]) - P * q0[1]);
  const det = a11 * a22 - a12 * a21;
  return [(b1 * a22 - a12 * b2) / det, (a11 * b2 - b1 * a21) / det];
}
function cloudPose(ch) {
  const [X, Y, z] = CLOUD[ch.i];
  const [px, py] = unproject(CLOUD_T, X, Y, z);
  const sgn = ch.i % 2 === 0 ? 1 : -1;
  return {
    x: r2(px - CARD4.x - ch.w / 2),
    y: r2(py - CARD4.y - 20),
    z,
    rotationX: sgn * (18 + ((ch.i * 7) % 17)),
    rotationY: -sgn * (40 + ((ch.i * 13) % 26)),
    rotation: sgn * (6 + ((ch.i * 5) % 12)),
  };
}
const drift = (p, k) => ({
  ...p,
  x: r2(p.x + 14 * k),
  y: r2(p.y - 9),
  rotationX: r3(p.rotationX * 0.8),
  rotationY: r3(p.rotationY * 0.85),
});
const POP4 = r3(T.dive[0] + 0.41);
const KEEP_AT = r3(T.dive[0] + 0.51);
const KEEP_STEP = 0.07;
const DROP_AT = r3(T.dive[0] + 0.57);
const DENY_AT = r3(T.dive[0] + 1.0);
// The denied chip docks just above the card's top-right corner, inside the frame and a touch
// larger than the cloud, and holds for ~2s after its Deny and saas.github pills land before it is
// flung away ahead of the crane.
const DENY_STOP = { x: 450, y: -108, z: 196, rotationX: 0, rotationY: 0, rotation: 0, scale: 1.1 };
const FLING_AT = r3(T.crane[0] - 0.38);
function card3Html() {
  const slots = SLOTS.map(([x, y]) => `<div class="f2-slot" style="${box(x, y, 354, 58)}"></div>`).join('');
  return `
          <div class="f2-card f2-card-pop" id="f2-card-mcp" style="width:780px;height:350px">
            <div class="f2-ibox" style="${box(28, 23)}">${icon('mcp', 'f2-ico-24')}</div>
            <div class="f2-abs f2-mut f2-lbl" style="${box(86, 22)}">Tool set</div>
            <div class="f2-abs f2-c4-name" style="${box(86, 42)}">triage</div>
            <div class="f2-abs f2-c4-count" style="right:28px;top:38px"><span class="f2-dmask"><span class="f2-dstrip" id="f2-c4-n"><span>0</span><span>1</span><span>2</span><span>3</span><span>4</span></span></span><span class="f2-mut">&nbsp;of 38 tools</span></div>
            <div class="f2-rule" style="top:90px"></div>${slots}
            <div class="f2-rule" style="top:266px"></div>
            <div class="f2-abs f2-mut f2-c4-foot" style="${box(28, 290)}">${SHIELD}<span><span class="f2-mono f2-fgc">tools/call</span> outside the set is rejected before upstream.</span></div>
            <div class="f2-c4-flash" id="f2-c4-flash"></div>
          </div>`;
}
function chipsHtml() {
  return CHIPS.map((ch) => {
    const chip = `<div class="f2-chip" style="width:${px(ch.w)}">${ch.role === 'deny' ? '<div class="f2-chip-red" id="f2-chip-red"></div>' : ''}<span class="f2-srv">${ch.srv}.</span><span class="f2-tool">${ch.tool}</span></div>`;
    const deny =
      ch.role === 'deny'
        ? `<div class="f2-denyrow"><span class="f2-pill f2-pill-deny" id="f2-deny">Deny</span><span class="f2-pill f2-pill-out f2-mono" id="f2-pack">saas.github</span></div>`
        : '';
    return `
          <div class="f2-chipg" id="f2-chip-${ch.i}" style="width:${px(ch.w)}">${chip}${deny}</div>`;
  }).join('');
}
function stop3Ops() {
  ft('#f2-cast-mcp', { opacity: 0 }, { opacity: 1, duration: 0.5, ease: 'power2.out' }, POP4);
  ft('#f2-card-mcp', { opacity: 0, y: 28, z: 150, scale: 0.97 }, { opacity: 1, y: 0, z: 150, scale: 1, duration: 0.5, ease: 'power3.out' }, POP4);
  let keep = 0;
  let drop = 0;
  for (const ch of CHIPS) {
    const sel = `#f2-chip-${ch.i}`;
    const A = cloudPose(ch);
    ft(sel, { opacity: 0 }, { opacity: 1, duration: 0.25, ease: 'none' }, T.dive[0] + 0.03 + ch.i * 0.015);
    if (ch.role === 'keep') {
      const at = KEEP_AT + keep * KEEP_STEP;
      const A2 = drift(A, ch.i % 2 ? -1 : 1);
      ft(sel, A, { ...A2, duration: r3(at - T.dive[0] - 0.001), ease: 'none' }, T.dive[0]);
      const [sx, sy] = SLOTS[keep];
      ft(sel, A2, { x: sx + 10, y: sy + 9, z: 152, rotationX: 0, rotationY: 0, rotation: 0, duration: 0.6, ease: 'power3.out' }, at);
      // The count ticks as each chip lands (~94% of its power3.out travel).
      ft('#f2-c4-n', { y: -22 * keep }, { y: -22 * (keep + 1), duration: 0.06, ease: 'power2.out' }, at + 0.28);
      // Docked chips leave with the card on the crane.
      ft(sel, { opacity: 1 }, { opacity: 0.4, duration: 0.4, ease: 'power2.in' }, T.crane[0]);
      keep++;
    } else if (ch.role === 'drop') {
      const at = DROP_AT + drop * 0.035;
      const A2 = drift(A, ch.i % 2 ? -1 : 1);
      ft(sel, A, { ...A2, duration: r3(at - T.dive[0] - 0.001), ease: 'none' }, T.dive[0]);
      // Sink back toward the page, easing outward from the card centre.
      const cx = CARD4.w / 2 - ch.w / 2;
      const cy = CARD4.h / 2 - 20;
      ft(
        sel,
        { ...A2, scale: 1 },
        {
          x: r2(cx + (A2.x - cx) * 1.08),
          y: r2(cy + (A2.y - cy) * 1.08),
          z: Math.max(12, A2.z - 260),
          rotationY: r3(A2.rotationY * 1.3),
          scale: 0.9,
          duration: 0.6,
          ease: 'power2.in',
        },
        at,
      );
      ft(sel, { opacity: 1 }, { opacity: 0, duration: 0.6, ease: 'power2.in' }, at);
      drop++;
    } else {
      const A2 = drift(A, -1);
      ft(sel, A, { ...A2, duration: r3(DENY_AT - T.dive[0] - 0.001), ease: 'none' }, T.dive[0]);
      ft(sel, { ...A2, scale: 1 }, { ...DENY_STOP, duration: 0.3, ease: 'power3.out' }, DENY_AT);
      ft('#f2-chip-red', { opacity: 0 }, { opacity: 1, duration: 0.12, ease: 'none' }, DENY_AT + 0.24);
      ft('#f2-c4-flash', { opacity: 0 }, { opacity: 1, duration: 0.08, ease: 'none' }, DENY_AT + 0.25);
      ft('#f2-c4-flash', { opacity: 1 }, { opacity: 0, duration: 0.4, ease: 'power2.out' }, DENY_AT + 0.33);
      ft('#f2-deny', { opacity: 0, y: 10 }, { opacity: 1, y: 0, duration: 0.4, ease: 'power3.out' }, DENY_AT + 0.27);
      ft('#f2-pack', { opacity: 0, y: 10 }, { opacity: 1, y: 0, duration: 0.4, ease: 'power3.out' }, DENY_AT + 0.33);
      // Rejection shake, then the chip is flung up and away.
      const xs = [DENY_STOP.x, DENY_STOP.x + 12, DENY_STOP.x - 9, DENY_STOP.x + 5, DENY_STOP.x];
      for (let k = 0; k < 4; k++) {
        ft(sel, { x: xs[k] }, { x: xs[k + 1], duration: 0.055, ease: 'sine.inOut' }, DENY_AT + 0.3 + k * 0.055);
      }
      ft(
        sel,
        { ...DENY_STOP },
        { x: 860, y: -470, z: 120, rotationX: -48, rotationY: 20, rotation: 26, duration: 0.5, ease: 'power2.in' },
        FLING_AT,
      );
      ft(sel, { opacity: 1 }, { opacity: 0, duration: 0.5, ease: 'power2.in' }, FLING_AT);
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Stop 7 — agent skills. incident-triage publishes v4: the version chip pops in, Latest slides
// over to it, and the CLI installs it into the agent's skills directory.
const SK_CMD = 'oceans skills install platform/incident-triage --version 4';
const SK_CH = 10.2; // JetBrains Mono 17px advance
const SK_V = [544, 594, 644, 694];
const SK_STEP = SK_V[1] - SK_V[0];
const SK_FILES = ['SKILL.md', 'references/checklist.md', '.oceans-skill-lock.json'];
const SK_TYPE_AT = T.crane[0] + 1.3;
function skillCardHtml() {
  const chips = SK_V.slice(0, 3)
    .map((x, i) => `<div class="f2-sk-v f2-mono" style="${box(x, 30)}">v${i + 1}</div>`)
    .join('');
  const files = SK_FILES.map((f, i) => `<span class="f2-sk-file" id="f2-sk-file-${i}">${huge('File01Icon', 'f2-ico-16')}<span class="f2-mono">${f}</span></span>`).join('');
  return `
          <div class="f2-card" id="f2-card-skill" style="width:880px;height:380px">
            <div class="f2-ibox" style="${box(28, 26)}">${huge('TaskDaily01Icon', 'f2-ico-22')}</div>
            <div class="f2-abs f2-mono f2-sk-name" style="${box(86, 22)}">incident-triage</div>
            <div class="f2-abs f2-mut f2-cm-desc" style="${box(86, 54)}">platform · Priya Shah</div>${chips}
            <div class="f2-sk-v f2-mono f2-sk-v4" id="f2-sk-v4" style="${box(SK_V[3], 30)}">v4</div>
            <div class="f2-sk-ring" id="f2-sk-ring" style="${box(SK_V[2] - 3, 27)}"><div class="f2-sk-rping" id="f2-sk-rping"></div></div>
            <div class="f2-sk-latest" id="f2-sk-latest" style="${box(702, 30)}"><span>Latest v</span><span class="f2-sk-dmask" ${MASKED}><span class="f2-sk-dstrip" id="f2-sk-dstrip"><span ${MASKED}>3</span><span ${MASKED}>4</span></span></span></div>
            <div class="f2-rule" style="top:92px"></div>
            <div class="f2-abs f2-sk-about" style="${box(28, 108, 824)}">Triage a production incident: pull gateway request logs, correlate trace IDs, and draft the status update.</div>
            <div class="f2-sk-term" style="${box(28, 172, 824, 120)}">
              <div class="f2-abs f2-mono f2-mut f2-sk-tl" style="${box(20, 22)}">$</div>
              <div class="f2-sk-type" id="f2-sk-type" style="${box(20 + 2 * SK_CH, 22)}" ${MASKED}><span class="f2-mono f2-sk-tl">${SK_CMD}</span></div>
              <div class="f2-sk-caret" id="f2-sk-caret" style="${box(20 + 2 * SK_CH, 26)}"></div>
              <div class="f2-abs f2-sk-l2" id="f2-sk-l2" style="${box(20, 64)}">${huge('Tick02Icon', 'f2-ico-16 f2-okc')}<span class="f2-mono f2-sk-tl">Installed .agents/skills/incident-triage</span></div>
            </div>
            <div class="f2-abs f2-sk-files" style="${box(28, 312)}">${files}</div>
          </div>`;
}
function skillOps() {
  // Publish: Latest slides aside, v4 pops into the gap, the digit rolls and the ring follows.
  const t0 = T.crane[0];
  ft('#f2-sk-latest', { x: 0 }, { x: SK_STEP, duration: 0.3, ease: 'power3.inOut' }, t0 + 1.05);
  ft('#f2-sk-v4', { opacity: 0, scale: 0.6 }, { opacity: 1, scale: 1, duration: 0.3, ease: 'back.out(2)' }, t0 + 1.17);
  ft('#f2-sk-ring', { x: 0 }, { x: SK_STEP, duration: 0.3, ease: 'power3.inOut' }, t0 + 1.2);
  ft('#f2-sk-dstrip', { y: 0 }, { y: -22, duration: 0.3, ease: 'back.out(1.4)' }, t0 + 1.25);
  ft('#f2-sk-rping', { opacity: 0.9, scale: 1 }, { opacity: 0, scale: 1.4, duration: 0.45, ease: 'power2.out', immediateRender: false }, t0 + 1.5);
  // Install: the command types out, the CLI confirms, and the bundle's files land.
  const n = SK_CMD.length;
  st('#f2-sk-caret', { opacity: 1 }, SK_TYPE_AT - 0.05);
  ft('#f2-sk-type', { width: 0 }, { width: r2(n * SK_CH), duration: 0.32, ease: `steps(${n})` }, SK_TYPE_AT);
  ft('#f2-sk-caret', { x: 0 }, { x: r2(n * SK_CH), duration: 0.32, ease: `steps(${n})` }, SK_TYPE_AT);
  st('#f2-sk-caret', { opacity: 0 }, t0 + 1.67);
  ft('#f2-sk-l2', { opacity: 0, y: 6 }, { opacity: 1, y: 0, duration: 0.25, ease: 'power2.out' }, t0 + 1.7);
  SK_FILES.forEach((_, i) =>
    ft(`#f2-sk-file-${i}`, { opacity: 0, y: 10 }, { opacity: 1, y: 0, duration: 0.3, ease: 'power3.out' }, t0 + 1.78 + i * 0.06),
  );
}

// ---------------------------------------------------------------------------------------------
// HUD rail: kicker + one-line claim per stop, vertical spring ticker (mask = item height).
const KICKS = [
  ['01', 'Model routing'],
  ['02', 'Client config'],
  ['03', 'Spend controls'],
  ['04', 'Secret redaction'],
  ['05', 'Destructive actions'],
  ['06', 'MCP tool sets'],
  ['07', 'Agent skills'],
];
const LINES = [
  ['Route', 'model.'],
  ['Connect', 'agent.'],
  ['Cap', 'dollar.'],
  ['Redact', 'secret.'],
  ['Deny', 'delete.'],
  ['Govern', 'tool.'],
  ['Share', 'skill.'],
];
const RAIL_H = 80;
const KICK_H = 26;
const TICKS = [T.orbit[0] + 0.05, T.whip + 0.1, T.swoop[0] + 0.2, T.pan[0] + 0.15, T.dive[0] + 0.1, T.crane[0] + 0.15];
st('#f2-kick-rise', { opacity: 1 }, 0.26);
ft('#f2-kick-rise', { y: 34 }, { y: 0, duration: 0.12, ease: 'power4.out' }, 0.26);
st('#f2-rail-rise', { opacity: 1 }, 0.32);
ft('#f2-rail-rise', { y: 70 }, { y: 0, duration: 0.18, ease: 'power4.out' }, 0.32);
TICKS.forEach((at, k) => {
  ft('#f2-kick-strip', { y: -KICK_H * k }, { y: -KICK_H * (k + 1), duration: 0.5, ease: 'back.out(1.4)' }, at - 0.04);
  ft('#f2-rail-strip', { y: -RAIL_H * k }, { y: -RAIL_H * (k + 1), duration: 0.5, ease: 'back.out(1.4)' }, at);
});
ft('#f2-kicker', { opacity: 1 }, { opacity: 0, duration: 0.25, ease: 'power1.in' }, T.exit[0] + 0.05);
ft('#f2-veil-fill', { opacity: 1 }, { opacity: 0, duration: 0.35, ease: 'power2.out' }, 0);

// Stop choreography. Each card lifts out of its source element; hc is the card row the source
// maps onto. The client card is the routing card's back face and the MCP card pops in with its
// chip cloud, so neither of those lifts.
const HC = { route: 50, client: 50, spend: 50, redact: STOPS[stopIndex('redact')].card.h / 2, cmd: 0, skill: 44 };
const LIFT_AT = { route: 0.35, spend: T.whip + 0.35, redact: T.swoop[0] + 0.5, cmd: T.pan[0] + 0.27, skill: T.crane[0] + 0.5 };
for (const [key, t0] of Object.entries(LIFT_AT)) {
  const s = STOPS[stopIndex(key)];
  liftOps(s, t0, liftHtml(s, HC[key]).from);
}
stop0Ops();
clientOps();
stop1Ops();
stop2Ops();
cmdOps();
stop3Ops();
skillOps();
departCard('client', T.whip);
departCard('spend', T.swoop[0]);
departCard('cmd', T.dive[0]);
departCard('mcp', T.crane[0]);
// The redaction card sinks back into the page as the camera pans across to the next widget.
ft('#f2-card-redact', { opacity: 1, z: 150 }, { opacity: 0, z: 60, duration: 0.35, ease: 'power2.in' }, T.pan[0]);
ft('#f2-cast-redact', { opacity: 1 }, { opacity: 0, duration: 0.35, ease: 'power2.in' }, T.pan[0]);

// ---------------------------------------------------------------------------------------------
// World HTML. Each page carries its stops: the lift surface (contact shadow and highlight ring),
// the ghost of the source element, and the card. On the models page the routing card and its
// client-config back face share one flip wrapper; the MCP card carries its chip cloud.
const CARD_HTML = {
  route: card0Html(),
  client: clientCardHtml(),
  spend: card1Html(),
  redact: card2Html(),
  cmd: cmdCardHtml(),
  mcp: card3Html(),
  skill: skillCardHtml(),
};
const FLIP_KEYS = ['route', 'client'];
function storyPlaneHtml(P, idx) {
  let surface = '';
  let ghosts = '';
  let flip = '';
  let cards = '';
  for (const st0 of STOPS.filter((s) => s.plane === idx)) {
    const { key, card } = st0;
    if (key === 'mcp') {
      surface += `<div class="f2-cast" id="f2-cast-mcp" style="${castBox(card)}"></div>`;
    } else {
      const L = liftHtml(st0, HC[key]);
      surface += L.surface;
      ghosts += L.ghost;
    }
    const wpos = `
        <div class="f2-wpos" style="${box(card.x, card.y, card.w, card.h)}">${CARD_HTML[key]}${key === 'mcp' ? chipsHtml() : ''}
        </div>`;
    if (FLIP_KEYS.includes(key)) flip += wpos;
    else cards += wpos;
  }
  if (flip) {
    flip = `
        <div class="f2-wpos" id="f2-flip" style="${box(0, 0, PW, PH)}">${flip}
        </div>${CURSOR_HTML}`;
  }
  return planeHtml(P, surface, `${ghosts}${flip}${cards}`, [8, 0.5]);
}
function planeHtml(P, surfaceExtra, layers, [dof, dim]) {
  const spans = cullSpans(P);
  const hidden = !spans.length || spans[0][0] > 0;
  const [x, y, z] = P.pos;
  return `
      <div class="f2-plane" id="f2-p-${P.id}" style="transform:translate3d(${x}px,${y}px,${z}px) rotateY(${P.a}deg);--f2-dof:${dof};--f2-dim:${dim}${hidden ? ';visibility:hidden' : ''}">
        <div class="f2-surface"><img class="f2-shot" src="../../assets/${P.img}" alt="" />${surfaceExtra}</div>${layers}
      </div>`;
}
const worldHtml = [
  ...PLANES.map((P, i) => storyPlaneHtml(P, i)),
  ...FILLERS.map((P) => planeHtml(P, '', '', [12, 0.35])),
].join('');
const dustHtml = keptDust(110)
  .map(
    (d) =>
      `<div class="f2-dust f2-dust-${d.tint}" style="width:${d.size}px;height:${d.size}px;margin:${-d.size / 2}px 0 0 ${-d.size / 2}px;opacity:${d.o.toFixed(3)};transform:translate3d(${Math.round(d.p[0])}px,${Math.round(d.p[1])}px,${Math.round(d.p[2])}px)"></div>`,
  )
  .join('\n      ');

const kickHtml = KICKS.map(([n, t]) => `<div class="f2-kick" ${MASKED}><span class="f2-kn" ${MASKED}>${n}</span>${t}</div>`).join('');
const lineHtml = LINES.map(([v, n]) => `<div class="f2-line" ${MASKED}>${v} <span class="f2-every" ${MASKED}>every</span> ${n}</div>`).join('');

const html = `<template>
  <script src="https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js"></script>
  <style>
    @font-face {
      font-family: "Geist";
      src: url("../../assets/fonts/geist-latin-wght-normal.woff2") format("woff2");
      font-weight: 100 900;
      font-style: normal;
      font-display: block;
    }
    @font-face {
      font-family: "JetBrains Mono";
      /* Official JetBrains Mono 2.304 webfont. Rules that use it turn ligatures off, so it renders as JetBrains Mono NL. */
      src: url("../../assets/fonts/jetbrains-mono-regular.woff2") format("woff2");
      font-weight: 400;
      font-style: normal;
      font-display: block;
    }
    #root {
      position: absolute;
      inset: 0;
      width: 1920px;
      height: 1080px;
      overflow: hidden;
      font-family: "Geist", sans-serif;
      font-weight: 500;
      color: oklch(0.985 0.004 226);
      --f2-fg: oklch(0.985 0.004 226);
      --f2-mfg: oklch(0.708 0.018 230);
      --f2-card-bg: oklch(0.205 0.016 238);
      --f2-secondary: oklch(0.274 0.016 238);
      --f2-muted: oklch(0.269 0.014 238);
      --f2-border: oklch(0.985 0.004 226 / 10%);
      --f2-border-strong: oklch(0.985 0.004 226 / 16%);
      --f2-primary: oklch(0.513 0.112 237.173);
      --f2-warn: oklch(0.769 0.156 77.727);
      --f2-warn-soft: color-mix(in oklch, oklch(0.769 0.156 77.727) 24%, transparent);
      --f2-destr: oklch(0.704 0.191 22.216);
      --f2-ok: oklch(0.696 0.17 162.48);
      --f2-wave: #0b83ad;
      --f2-mist: #b7dde4;
    }
    #f2-bg {
      position: absolute;
      inset: 0;
      background: radial-gradient(ellipse 70% 70% at 50% 45%, oklch(0.18 0.02 238) 0%, oklch(0.145 0.012 238) 55%, oklch(0.115 0.01 238) 100%);
    }
    #f2-stage {
      position: absolute;
      inset: 0;
      overflow: hidden;
    }
    #f2-fx {
      position: absolute;
      width: 0;
      height: 0;
    }
    #f2-optics {
      position: absolute;
      inset: 0;
      filter: url(#f2-mb);
    }
    #f2-shake {
      position: absolute;
      inset: 0;
    }
    #f2-lens {
      position: absolute;
      inset: 0;
      perspective: 1000px;
      perspective-origin: 50% 50%;
    }
    .f2-rig {
      position: absolute;
      inset: 0;
      transform-style: preserve-3d;
    }
    .f2-plane {
      position: absolute;
      left: 160px;
      top: 90px;
      width: 1600px;
      height: 900px;
      transform-style: preserve-3d;
    }
    .f2-surface {
      position: absolute;
      inset: 0;
      transform: translateZ(0px);
    }
    .f2-shot {
      position: absolute;
      left: 0;
      top: 0;
      width: 1600px;
      height: 900px;
      display: block;
      border-radius: 12px;
      box-shadow: 0 0 0 1px oklch(1 0 0 / 9%);
      filter: blur(calc(var(--f2-dof) * 1px)) brightness(var(--f2-dim));
    }
    .f2-cast {
      position: absolute;
      border-radius: 40px;
      background: oklch(0.07 0.01 238 / 0.8);
      box-shadow: 0 0 60px 34px oklch(0.07 0.01 238 / 0.8);
      opacity: 0;
    }
    .f2-hl {
      position: absolute;
      border-radius: 10px;
      box-shadow: 0 0 0 2px var(--f2-wave), 0 0 30px 2px oklch(0.62 0.12 228 / 0.5);
      opacity: 0;
    }
    .f2-ghost {
      position: absolute;
      border-radius: 6px;
      background-repeat: no-repeat;
      background-size: 1600px 900px;
      box-shadow: 0 16px 34px oklch(0 0 0 / 0.5);
      backface-visibility: hidden;
      opacity: 0;
    }
    .f2-wpos {
      position: absolute;
      transform-style: preserve-3d;
    }
    .f2-card {
      position: absolute;
      left: 0;
      top: 0;
      transform-origin: 0 0;
      border-radius: 16px;
      background: var(--f2-card-bg);
      box-shadow: inset 0 0 0 1px var(--f2-border), inset 0 1px 0 oklch(1 0 0 / 6%);
      overflow: hidden;
      backface-visibility: hidden;
      opacity: 0;
    }
    .f2-card-pop {
      transform-origin: 50% 50%;
    }
    .f2-abs {
      position: absolute;
      white-space: nowrap;
    }
    .f2-mono {
      font-family: "JetBrains Mono", monospace;
      font-variant-ligatures: none;
      font-weight: 400;
    }
    .f2-mut {
      color: var(--f2-mfg);
    }
    .f2-fgc {
      color: var(--f2-fg);
    }
    .f2-lbl {
      font-size: 15px;
      line-height: 20px;
    }
    .f2-rule {
      position: absolute;
      left: 28px;
      right: 28px;
      height: 1px;
      background: var(--f2-border);
    }
    .f2-ibox {
      position: absolute;
      width: 44px;
      height: 44px;
      border-radius: 10px;
      background: var(--f2-secondary);
      display: flex;
      align-items: center;
      justify-content: center;
      color: var(--f2-fg);
    }
    .f2-ibox-plain {
      background: none;
      width: 40px;
      height: 40px;
    }
    .f2-ico-24 { width: 24px; height: 24px; display: block; }
    .f2-ico-26 { width: 26px; height: 26px; display: block; }
    .f2-ico-34 { width: 34px; height: 34px; display: block; }
    .f2-ico-18 { width: 18px; height: 18px; display: block; flex: none; }
    .f2-ico-16 { width: 16px; height: 16px; display: block; flex: none; }
    .f2-ico-22 { width: 22px; height: 22px; display: block; }
    .f2-okc { color: var(--f2-ok); }
    .f2-destrc { color: var(--f2-destr); }
    .f2-title { font-size: 24px; line-height: 30px; }
    .f2-pill {
      display: inline-flex;
      align-items: center;
      height: 24px;
      padding: 0 10px;
      border-radius: 999px;
      font-size: 13px;
      line-height: 24px;
      white-space: nowrap;
    }
    .f2-pill-sec { background: var(--f2-secondary); color: var(--f2-fg); }
    .f2-pill-out { box-shadow: inset 0 0 0 1px var(--f2-border-strong); color: var(--f2-mfg); }
    .f2-pill-warn { background: var(--f2-warn-soft); color: var(--f2-warn); }
    .f2-pill-deny { background: oklch(0.704 0.191 22.216 / 0.2); color: var(--f2-destr); }
    .f2-pills {
      display: flex;
      gap: 8px;
    }
    .f2-pills .f2-pill {
      height: 28px;
      line-height: 28px;
      padding: 0 12px;
      font-size: 14px;
    }
    .f2-track {
      position: absolute;
      border-radius: 4px;
      background: var(--f2-muted);
      overflow: hidden;
    }
    .f2-fill {
      position: absolute;
      inset: 0;
      border-radius: 4px;
      background: var(--f2-primary);
      transform-origin: 0 50%;
      transform: scaleX(0);
    }
    /* Stop 1 */
    .f2-c1-model { height: 52px; line-height: 52px; font-size: 26px; }
    .f2-c1-policy { text-align: right; font-size: 20px; line-height: 28px; }
    .f2-sel {
      position: absolute;
      left: 14px;
      width: 892px;
      height: 64px;
      border-radius: 12px;
      box-shadow: inset 0 0 0 2px var(--f2-wave);
      background: oklch(0.6 0.11 230 / 0.09);
      opacity: 0;
    }
    .f2-ping { background: none; }
    .f2-row {
      position: absolute;
      left: 0;
      width: 920px;
      height: 72px;
      opacity: 0;
    }
    .f2-row-in { position: absolute; inset: 0; }
    .f2-rname { display: flex; align-items: center; gap: 10px; height: 28px; font-size: 20px; }
    .f2-rid { font-size: 15px; line-height: 20px; }
    .f2-pw { font-size: 14px; line-height: 18px; }
    .f2-pct { text-align: right; font-size: 24px; line-height: 32px; font-variant-numeric: tabular-nums; }
    /* Stop 2 */
    .f2-avatar {
      position: absolute;
      width: 52px;
      height: 52px;
      border-radius: 50%;
      background: #006f9f;
      color: var(--f2-fg);
      font-size: 24px;
      line-height: 52px;
      text-align: center;
    }
    .f2-c2-name { font-size: 22px; line-height: 28px; }
    .f2-c2-mail { font-size: 16px; line-height: 22px; }
    .f2-status {
      position: absolute;
      width: 132px;
      height: 32px;
      border-radius: 999px;
      overflow: hidden;
    }
    .f2-status-bg { position: absolute; inset: 0; background: var(--f2-secondary); }
    .f2-status-warn { background: var(--f2-warn-soft); opacity: 0; }
    .f2-status-mask { position: absolute; inset: 0; overflow: hidden; }
    #f2-c2-status div { height: 32px; line-height: 32px; text-align: center; font-size: 15px; color: var(--f2-fg); }
    #f2-c2-status .f2-warn-t { color: var(--f2-warn); }
    .f2-status-ping {
      position: absolute;
      width: 132px;
      height: 32px;
      border-radius: 999px;
      box-shadow: 0 0 0 2px var(--f2-warn);
      opacity: 0;
    }
    .f2-c2-lbl { font-size: 16px; line-height: 22px; }
    .f2-c2-amtrow { display: flex; align-items: baseline; gap: 12px; }
    .f2-c2-amt { font-size: 48px; line-height: 56px; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
    .f2-c2-of { font-size: 20px; }
    .f2-track-lg { border-radius: 5px; }
    .f2-fillwrap { position: absolute; inset: 0; transform-origin: 0 50%; transform: scaleX(0); }
    .f2-fill-l { position: absolute; inset: 0; border-radius: 5px; }
    .f2-fill-low { background: oklch(0.708 0.018 230 / 0.6); }
    .f2-fill-pri { background: var(--f2-primary); opacity: 0; }
    .f2-fill-warn { background: var(--f2-warn); opacity: 0; }
    .f2-tick { position: absolute; width: 2px; height: 22px; border-radius: 1px; background: oklch(0.985 0.004 226 / 45%); }
    .f2-tick-l { text-align: center; font-size: 12px; line-height: 16px; font-variant-numeric: tabular-nums; }
    /* Stop 3 */
    .f2-c3-head { display: flex; align-items: center; gap: 10px; font-size: 15px; }
    .f2-c3-head .f2-pill { height: 28px; padding: 0 12px; font-size: 14px; }
    .f2-c3-tag { display: flex; align-items: center; gap: 8px; font-size: 15px; line-height: 24px; }
    .f2-c3-msg { font-size: 22px; line-height: 30px; }
    .f2-c3-env { font-size: 28px; line-height: 44px; color: oklch(0.985 0.004 226 / 80%); }
    .f2-band { position: absolute; border-radius: 9px; }
    .f2-band-red { background: oklch(0.704 0.191 22.216 / 0.16); box-shadow: inset 0 0 0 1px oklch(0.704 0.191 22.216 / 0.4); }
    .f2-band-wave { background: oklch(0.6 0.11 230 / 0.16); box-shadow: inset 0 0 0 1px oklch(0.6 0.11 230 / 0.45); opacity: 0; }
    .f2-keyline { position: absolute; width: 900px; height: 44px; }
    .f2-cell {
      position: absolute;
      top: 0;
      width: ${CELL}px;
      height: 44px;
      perspective: 160px;
    }
    .f2-g {
      position: absolute;
      inset: 0;
      text-align: center;
      font-family: "JetBrains Mono", monospace;
      font-variant-ligatures: none;
      font-weight: 400;
      font-size: 28px;
      line-height: 44px;
      transform-origin: 50% 100%;
      backface-visibility: hidden;
    }
    .f2-g-old { color: var(--f2-destr); }
    .f2-g-fl { color: var(--f2-wave); opacity: 0; }
    .f2-g-new { color: var(--f2-mist); opacity: 0; }
    /* Stop 4 */
    .f2-c4-name { font-size: 26px; line-height: 32px; }
    .f2-c4-count { display: flex; align-items: flex-end; font-size: 16px; line-height: 22px; font-variant-numeric: tabular-nums; }
    .f2-dmask { display: inline-block; height: 22px; overflow: hidden; }
    .f2-dstrip { display: block; }
    .f2-dstrip span { display: block; height: 22px; line-height: 22px; }
    .f2-slot {
      position: absolute;
      border-radius: 12px;
      border: 1.5px dashed oklch(0.985 0.004 226 / 16%);
    }
    .f2-c4-foot { display: flex; align-items: center; gap: 10px; font-size: 16px; line-height: 24px; }
    .f2-c4-flash {
      position: absolute;
      inset: 0;
      border-radius: 16px;
      box-shadow: inset 0 0 0 2px var(--f2-destr), inset 0 0 40px oklch(0.704 0.191 22.216 / 0.25);
      opacity: 0;
    }
    .f2-chipg {
      position: absolute;
      left: 0;
      top: 0;
      backface-visibility: hidden;
      opacity: 0;
    }
    .f2-chip {
      position: relative;
      height: 40px;
      padding: 0 16px;
      display: flex;
      align-items: center;
      border-radius: 10px;
      background: oklch(0.24 0.016 238);
      box-shadow: inset 0 0 0 1px oklch(1 0 0 / 12%), 0 10px 24px oklch(0 0 0 / 0.35);
      font-family: "JetBrains Mono", monospace;
      font-variant-ligatures: none;
      font-weight: 400;
      font-size: 18px;
      line-height: 40px;
      white-space: nowrap;
      overflow: hidden;
    }
    .f2-srv, .f2-tool { position: relative; }
    .f2-srv { color: var(--f2-mfg); }
    .f2-chip-red {
      position: absolute;
      inset: 0;
      border-radius: 10px;
      background: oklch(0.704 0.191 22.216 / 0.2);
      box-shadow: inset 0 0 0 1.5px var(--f2-destr);
      opacity: 0;
    }
    .f2-denyrow { position: absolute; left: 0; top: 50px; display: flex; gap: 8px; }
    .f2-denyrow .f2-pill { height: 28px; line-height: 28px; padding: 0 12px; font-size: 14px; opacity: 0; }
    /* Client config */
    .f2-cursor {
      position: absolute;
      transform-origin: 3px 2px;
      opacity: 0;
    }
    .f2-cursor svg {
      display: block;
      width: 22px;
      height: 30px;
      fill: var(--f2-fg);
      stroke: oklch(0.145 0.012 238);
      stroke-width: 1.6;
      stroke-linejoin: round;
      filter: drop-shadow(0 4px 6px oklch(0 0 0 / 0.45));
    }
    .f2-cc-title, .f2-cm-title { font-size: 24px; line-height: 30px; }
    .f2-cc-desc, .f2-cm-desc { font-size: 15px; line-height: 20px; }
    .f2-cc-tabs { position: absolute; border-radius: 10px; background: var(--f2-muted); }
    .f2-cc-trow { position: absolute; left: 4px; top: 4px; display: flex; height: 36px; }
    .f2-cc-tab {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 8px;
      width: 150px;
      height: 36px;
      font-size: 16px;
      line-height: 20px;
      white-space: nowrap;
    }
    .f2-cc-ind {
      position: absolute;
      left: 4px;
      top: 4px;
      width: 150px;
      height: 36px;
      border-radius: 7px;
      background: oklch(0.32 0.016 238);
      box-shadow: 0 1px 3px oklch(0 0 0 / 0.35);
      overflow: hidden;
    }
    .f2-cc-ind .f2-cc-trow { left: 0; top: 0; color: var(--f2-fg); }
    .f2-cc-pathrow { display: flex; align-items: center; gap: 12px; font-size: 15px; line-height: 24px; }
    .f2-cc-pmask { display: block; height: 24px; overflow: hidden; }
    .f2-cc-pstrip { display: block; }
    .f2-cc-pstrip span { display: block; height: 24px; line-height: 24px; color: var(--f2-fg); }
    .f2-cc-panel {
      position: absolute;
      border-radius: 12px;
      background: oklch(0.165 0.014 238);
      box-shadow: inset 0 0 0 1px var(--f2-border);
      overflow: hidden;
    }
    .f2-cc-reveal { position: absolute; inset: 0; }
    .f2-cc-strip { position: absolute; left: 0; top: 0; width: ${3 * CC_W}px; height: 312px; }
    .f2-cc-page { position: absolute; top: 0; width: ${CC_W}px; height: 312px; }
    .f2-cc-file { left: 16px; top: 0; height: 44px; display: flex; align-items: center; gap: 8px; font-size: 15px; line-height: 20px; }
    .f2-cc-hrule { position: absolute; left: 0; right: 0; top: 44px; height: 1px; background: var(--f2-border); }
    .f2-cc-row { position: absolute; left: 0; width: ${CC_W}px; height: ${CC_ROWH}px; }
    .f2-cc-ln, .f2-cc-code {
      position: absolute;
      top: 0;
      font-family: "JetBrains Mono", monospace;
      font-variant-ligatures: none;
      font-weight: 400;
      line-height: ${CC_ROWH}px;
      white-space: pre;
    }
    .f2-cc-ln { left: 16px; width: 36px; text-align: right; font-size: 15px; color: oklch(0.708 0.018 230 / 70%); }
    .f2-cc-code { left: 64px; font-size: 16px; }
    .f2-cc-fold {
      display: inline-block;
      height: 20px;
      padding: 0 8px;
      border-radius: 5px;
      background: oklch(0.985 0.004 226 / 6%);
      font-size: 14px;
      line-height: 20px;
      color: var(--f2-mfg);
      vertical-align: middle;
    }
    .f2-cc-band {
      position: absolute;
      left: 8px;
      width: 848px;
      height: ${CC_ROWH}px;
      border-radius: 6px;
      background: oklch(0.6 0.11 230 / 0.12);
      box-shadow: inset 3px 0 0 var(--f2-wave);
      transform-origin: 0 50%;
      opacity: 0;
    }
    .f2-cc-scan {
      position: absolute;
      left: 0;
      right: 0;
      top: 44px;
      height: 2px;
      background: var(--f2-wave);
      box-shadow: 0 0 14px 2px oklch(0.62 0.12 228 / 0.6);
      opacity: 0;
    }
    .f2-cc-copy {
      position: absolute;
      right: 12px;
      top: 8px;
      width: 116px;
      height: 28px;
      border-radius: 7px;
      background: var(--f2-secondary);
      box-shadow: inset 0 0 0 1px var(--f2-border);
    }
    .f2-cc-cmask { position: absolute; inset: 0; overflow: hidden; border-radius: 7px; }
    .f2-cc-crow { display: flex; align-items: center; justify-content: center; gap: 6px; height: 28px; font-size: 14px; line-height: 20px; }
    .f2-cc-cping {
      position: absolute;
      right: 12px;
      top: 8px;
      width: 116px;
      height: 28px;
      border-radius: 7px;
      box-shadow: 0 0 0 2px var(--f2-ok);
      opacity: 0;
    }
    .f2-jk { color: var(--f2-fg); }
    .f2-js { color: var(--f2-mist); }
    .f2-jl { color: oklch(0.74 0.1 228); }
    .f2-jp { color: var(--f2-mfg); }
    /* Destructive actions */
    #f2-card-cmd .f2-cm-title { font-size: 29px; line-height: 36px; }
    #f2-card-cmd .f2-cm-desc { font-size: 18px; line-height: 24px; }
    #f2-card-cmd .f2-lbl { font-size: 18px; line-height: 24px; }
    #f2-card-cmd .f2-rule { left: 34px; right: 34px; }
    #f2-card-cmd .f2-ibox { width: 53px; height: 53px; border-radius: 12px; }
    #f2-card-cmd .f2-ico-22 { width: 26px; height: 26px; }
    #f2-card-cmd .f2-ico-18 { width: 22px; height: 22px; }
    .f2-cm-tog { position: absolute; right: 34px; top: 38px; width: 192px; height: 43px; border-radius: 11px; background: var(--f2-muted); }
    .f2-cm-slots { position: absolute; left: 5px; top: 5px; display: flex; height: 34px; }
    .f2-cm-slots span { width: 91px; height: 34px; line-height: 34px; text-align: center; font-size: 18px; }
    .f2-cm-ind {
      position: absolute;
      left: 5px;
      top: 5px;
      width: 91px;
      height: 34px;
      border-radius: 7px;
      box-shadow: 0 1px 3px oklch(0 0 0 / 0.35);
      overflow: hidden;
    }
    .f2-cm-ind .f2-cm-slots { left: 0; top: 0; }
    .f2-cm-indbg { position: absolute; inset: 0; background: oklch(0.32 0.016 238); }
    .f2-cm-indred { background: oklch(0.704 0.191 22.216 / 0.2); opacity: 0; }
    .f2-cm-row { position: absolute; left: 0; width: 1080px; height: 67px; opacity: 0; }
    .f2-cm-rflash {
      position: absolute;
      left: 17px;
      right: 17px;
      top: 0;
      bottom: 0;
      border-radius: 12px;
      background: oklch(0.704 0.191 22.216 / 0.14);
      opacity: 0;
    }
    .f2-cm-line { top: 0; font-size: 24px; line-height: 67px; }
    .f2-cm-type { position: absolute; left: 72px; top: 0; width: 0; height: 67px; overflow: hidden; }
    .f2-cm-cmd { display: block; white-space: pre; color: var(--f2-fg); }
    .f2-cm-caret { position: absolute; left: 72px; top: 19px; width: 2px; height: 29px; background: var(--f2-wave); opacity: 0; }
    .f2-cm-strike {
      position: absolute;
      left: 67px;
      top: 32px;
      height: 2px;
      border-radius: 1px;
      background: var(--f2-destr);
      transform-origin: 0 50%;
      transform: scaleX(0);
    }
    .f2-cm-tags { position: absolute; right: 34px; top: 17px; display: flex; gap: 10px; }
    .f2-cm-tags .f2-pill { height: 34px; line-height: 34px; padding: 0 14px; font-size: 17px; opacity: 0; }
    .f2-cm-foot { display: flex; align-items: center; gap: 12px; font-size: 19px; line-height: 29px; opacity: 0; }
    .f2-cm-shield { display: flex; }
    .f2-cm-rmask { display: block; height: 29px; overflow: hidden; }
    .f2-cm-rstrip { display: block; font-size: 18px; }
    .f2-cm-rstrip span { display: block; height: 29px; line-height: 29px; }
    /* Agent skills */
    .f2-sk-name { font-size: 24px; line-height: 30px; }
    .f2-sk-v {
      position: absolute;
      width: 42px;
      height: 28px;
      border-radius: 7px;
      box-shadow: inset 0 0 0 1px var(--f2-border-strong);
      font-size: 14px;
      line-height: 28px;
      text-align: center;
      color: var(--f2-mfg);
    }
    .f2-sk-v4 { color: var(--f2-fg); opacity: 0; }
    .f2-sk-latest {
      position: absolute;
      width: 100px;
      height: 28px;
      border-radius: 999px;
      background: oklch(0.26 0.035 232);
      box-shadow: inset 0 0 0 1px oklch(0.6 0.11 230 / 0.45);
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 14px;
      line-height: 22px;
      color: var(--f2-mist);
      white-space: nowrap;
    }
    .f2-sk-dmask { display: block; height: 22px; overflow: hidden; }
    .f2-sk-dstrip { display: block; }
    .f2-sk-dstrip span { display: block; height: 22px; line-height: 22px; }
    .f2-sk-ring { position: absolute; width: 48px; height: 34px; border-radius: 9px; box-shadow: 0 0 0 2px var(--f2-wave); }
    .f2-sk-rping { position: absolute; inset: 0; border-radius: 9px; box-shadow: 0 0 0 2px var(--f2-wave); opacity: 0; }
    .f2-sk-about { font-size: 17px; line-height: 24px; white-space: normal; color: var(--f2-mfg); }
    .f2-sk-term {
      position: absolute;
      border-radius: 12px;
      background: oklch(0.165 0.014 238);
      box-shadow: inset 0 0 0 1px var(--f2-border);
      overflow: hidden;
    }
    .f2-sk-tl { font-size: 17px; line-height: 28px; white-space: pre; }
    .f2-sk-type { position: absolute; width: 0; height: 28px; overflow: hidden; }
    .f2-sk-type .f2-sk-tl { display: block; color: var(--f2-fg); }
    .f2-sk-caret { position: absolute; width: 2px; height: 20px; background: var(--f2-wave); opacity: 0; }
    .f2-sk-l2 { display: flex; align-items: center; gap: 8px; opacity: 0; }
    .f2-sk-files { display: flex; gap: 8px; }
    .f2-sk-file {
      display: flex;
      align-items: center;
      gap: 8px;
      height: 34px;
      padding: 0 12px;
      border-radius: 8px;
      background: var(--f2-secondary);
      box-shadow: inset 0 0 0 1px var(--f2-border);
      font-size: 15px;
      line-height: 20px;
      color: var(--f2-fg);
      opacity: 0;
    }
    .f2-sk-file svg { color: var(--f2-mfg); }
    .f2-dust {
      position: absolute;
      left: 960px;
      top: 540px;
      background-position: center;
      background-size: 100% 100%;
      background-repeat: no-repeat;
    }
    .f2-dust-wave { background-image: url("../../assets/fx/dust-wave.svg"); }
    .f2-dust-mist { background-image: url("../../assets/fx/dust-mist.svg"); }
    /* HUD */
    #f2-hud {
      position: absolute;
      inset: 0;
    }
    /* Scrim colours are written as rgba (rgb(2 4 5) = oklch(0.1 0.01 238), rgb(2 6 8) =
       oklch(0.115 0.012 238)) so the layout audit scores their real alpha; the caption pool is
       boxed to the ellipse it paints so it only counts as covering what it actually darkens. */
    #f2-vignette {
      position: absolute;
      inset: 0;
      background: radial-gradient(ellipse 80% 80% at 50% 45%, rgba(2, 4, 5, 0) 60%, rgba(2, 4, 5, 0.5) 100%);
    }
    #f2-pool {
      position: absolute;
      left: 0;
      bottom: 0;
      width: 1060px;
      height: 400px;
      background: radial-gradient(ellipse 1060px 400px at 0% 100%, rgba(2, 6, 8, 0.86) 0%, rgba(2, 6, 8, 0.55) 45%, rgba(2, 6, 8, 0) 100%);
    }
    #f2-kicker {
      position: absolute;
      left: 124px;
      top: 750px;
      height: ${KICK_H}px;
      overflow: hidden;
    }
    #f2-kick-rise { opacity: 0; }
    .f2-kick {
      height: ${KICK_H}px;
      line-height: ${KICK_H}px;
      font-family: "JetBrains Mono", monospace;
      font-variant-ligatures: none;
      font-weight: 400;
      font-size: 20px;
      letter-spacing: 0.12em;
      text-transform: uppercase;
      color: var(--f2-mfg);
      white-space: nowrap;
    }
    .f2-kn { color: var(--f2-wave); margin-right: 0.9em; }
    #f2-rail {
      position: absolute;
      left: 120px;
      top: 790px;
      height: ${RAIL_H}px;
      overflow: hidden;
    }
    #f2-rail-rise { opacity: 0; }
    .f2-line {
      height: ${RAIL_H}px;
      line-height: ${RAIL_H}px;
      font-size: 64px;
      letter-spacing: -0.03em;
      white-space: nowrap;
      color: var(--f2-fg);
    }
    .f2-every { color: var(--f2-wave); }
    #f2-veil {
      position: absolute;
      inset: 0;
    }
    #f2-veil-fill {
      position: absolute;
      inset: 0;
      background: #0a6192;
      opacity: 1;
    }
  </style>

  <div id="root" data-composition-id="f2-flight" data-width="1920" data-height="1080">
    <div id="f2-bg" class="clip" data-start="0" data-duration="${CUT}" data-track-index="0"></div>

    <div id="f2-stage" class="clip" data-start="0" data-duration="${CUT}" data-track-index="1">
      <svg id="f2-fx" width="0" height="0" aria-hidden="true" focusable="false">
        <filter id="f2-mb" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB">
          <feGaussianBlur id="f2-mbg" stdDeviation="20 20" />
        </filter>
      </svg>
      <div id="f2-optics">
        <div id="f2-shake">
          <div id="f2-lens">
            <div id="f2-drift" class="f2-rig">
              <div id="f2-cam" class="f2-rig" style="transform:${camInline.cam}">
                <div id="f2-pitch" class="f2-rig" style="transform:${camInline.pitch}">
                  <div id="f2-yaw" class="f2-rig" style="transform:${camInline.yaw}">
                    <div id="f2-world" class="f2-rig" style="transform:${camInline.world}" data-layout-allow-overflow>${worldHtml}
      ${dustHtml}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div id="f2-hud" class="clip" data-start="0" data-duration="${CUT}" data-track-index="2">
      <div id="f2-vignette"></div>
      <div id="f2-pool"></div>
      <div id="f2-kicker"><div id="f2-kick-rise" data-layout-allow-overflow><div id="f2-kick-strip">${kickHtml}</div></div></div>
      <div id="f2-rail"><div id="f2-rail-rise" data-layout-allow-overflow><div id="f2-rail-strip">${lineHtml}</div></div></div>
    </div>

    <div id="f2-veil" class="clip" data-start="0" data-duration="${CUT}" data-track-index="3"><div id="f2-veil-fill"></div></div>
  </div>

  <script>
    (function () {
      window.__timelines = window.__timelines || {};
      var tl = gsap.timeline({ paused: true });

      ${emitOps().join('\n      ')}

      tl.set({}, {}, ${CUT});
      window.__timelines['f2-flight'] = tl;
    })();
  </script>
</template>
`;

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, html);
console.log(`wrote ${path.relative(ROOT, OUT)} (${html.length} bytes, ${OPS.length} timeline ops)`);

// Diagnostic: where the chip cloud lands on screen (CHIPS=1 node .scratch/build-f2.mjs).
if (process.env.CHIPS) {
  const at = (t, p) => view(camAt(t), planePoint(STOPS[MCP].plane, CARD4.x + p.x, CARD4.y + p.y, p.z));
  for (const ch of CHIPS) {
    const A = cloudPose(ch);
    const c = { x: A.x + ch.w / 2, y: A.y + 20, z: A.z };
    const [x, y, k] = project(at(CLOUD_T, c), camAt(CLOUD_T).P);
    const half = (ch.w / 2) * k;
    const off = x - half < 0 || x + half > 1920 || y - 20 * k < 0 || y + 20 * k > 1080;
    let zmax = -1e9;
    for (let t = T.dive[0]; t <= T.dive[0] + 0.65; t += 1 / 30) zmax = Math.max(zmax, at(t, c)[2]);
    console.log(
      `${ch.role.padEnd(4)} ${`${ch.srv}.${ch.tool}`.padEnd(28)} (${Math.round(x - half)}..${Math.round(x + half)}, ${Math.round(y)}) k${k.toFixed(2)} viewz-max ${Math.round(zmax)}${off ? '  OFF' : ''}`,
    );
  }
  // The denied chip and its pill row across the hold, as on-screen boxes.
  const deny = CHIPS.find((ch) => ch.role === 'deny');
  for (const t of [DENY_AT + 0.35, DENY_AT + 1.2, FLING_AT - 0.01]) {
    const pose = camAt(t);
    const sc = (v, o) => o + (v - o) * DENY_STOP.scale; // the group scales about the chip's centre
    const pt = (x, y) => project(at(t, { x: DENY_STOP.x + sc(x, deny.w / 2), y: DENY_STOP.y + sc(y, 20), z: DENY_STOP.z }), pose.P);
    const [a, b] = [pt(0, 0), pt(deny.w, 78)];
    console.log(`deny @${t.toFixed(2)}: chip+pills (${Math.round(a[0])},${Math.round(a[1])})..(${Math.round(b[0])},${Math.round(b[1])}) k${a[2].toFixed(2)}`);
  }
}
