#!/usr/bin/env node
// Builds compositions/frames/f3-lockup.html — Frame 3, "Lockup".
// Out of the flight's blur, sixteen real admin panels assemble into a tilted wall while the
// camera pulls back, and the rail ticks to "See every request.". The wall then dims and
// defocuses, the Oceans mark lands like a drop (two water rings) and glides aside while the
// wordmark rises letter by letter, the rail's "every" flies into the tagline and the words ripple
// out from where it lands, and the URL and licence pills pop in. Wall geometry and the camera come from
// f3-geom, so the numbers the checker verified are the numbers rendered. Text metrics measured
// in the browser live in f3-metrics.json; the defaults below are estimates.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { TILES, W, H, DUR, FLY, FLY_EASE, CAM_FROM, CAM_TO, CAM_EASE } from './f3-geom.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT = path.join(ROOT, 'compositions/frames/f3-lockup.html');
const METRICS_FILE = path.join(ROOT, '.scratch/f3-metrics.json');
const ICON_DIR = path.resolve(
  ROOT,
  '../../crates/admin-ui/web/node_modules/@lobehub/icons-static-svg/icons',
);

const M = {
  wordW: 565, // "Oceans LLM" box width at WORD_PX
  wordCy: 66, // visual centre of the wordmark's ink inside its WORD_LH line box
  // left edge of each glyph in the kerned run (null for the space), so split letters keep kerning
  wordXs: [0, 76, 131, 188, 243, 300, null, 375, 429, 482],
  railEveryL: 118, // offsetLeft of "every" in "See every request." inside #f3-rail
  tagEveryL: 470, // offsetLeft of the tagline's first "every" inside #f3-tag
  ...(fs.existsSync(METRICS_FILE) ? JSON.parse(fs.readFileSync(METRICS_FILE, 'utf8')) : {}),
};

const GITHUB = fs
  .readFileSync(path.join(ICON_DIR, 'github.svg'), 'utf8')
  .replace(/^[\s\S]*?<\/title>/, '')
  .replace(/<\/svg>\s*$/, '')
  .trim();

const r2 = (v) => Math.round(v * 100) / 100;
const r3 = (v) => Math.round(v * 1000) / 1000;
const px = (v) => `${r2(v)}px`;

// ---------------------------------------------------------------------------------------------
// Timeline ops (same DSL as Frames 1–2). The first fromTo in time on a selector keeps
// immediateRender, so its "from" is the element's state before the tween; every later fromTo
// gets immediateRender:false, and emitOps warns when its "from" disagrees with the value the
// property rests at by then (CSS, a set, or the previous tween's end).
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
const val = (vars, k) => (k.startsWith('attr.') ? vars.attr[k.slice(5)] : vars[k]);
const BASELINE = { opacity: 1, x: 0, y: 0, z: 0, scale: 1, rotationX: 0, rotationY: 0, rotation: 0, xPercent: 0, yPercent: 0 };
// Properties whose stylesheet value differs from BASELINE.
const CSS_REST = {
  '#f3-logo|opacity': 0,
  '#f3-rp0|opacity': 0,
  '#f3-rp1|opacity': 0,
  '#f3-glow|opacity': 0,
  '#f3-shade|opacity': 0,
  '#f3-fly|opacity': 0,
  '#f3-pill-url|opacity': 0,
  '#f3-pill-oss|opacity': 0,
  '.f3-fr0|opacity': 0,
  '.f3-fr1|opacity': 0,
  '.f3-fr2|opacity': 0,
};
function emitOps() {
  const seenSel = new Set();
  const rest = new Map();
  const out = [];
  for (const op of OPS.slice().sort((a, b) => a.at - b.at || a.seq - b.seq)) {
    const vars = op.kind === 'set' ? op.vars : op.to;
    const props = propKeys(vars);
    if (op.kind === 'fromTo') {
      const to = { ...op.to };
      if (!('immediateRender' in to) && seenSel.has(op.sel)) {
        to.immediateRender = false;
        for (const k of props) {
          const key = `${op.sel}|${k}`;
          const now = rest.has(key) ? rest.get(key) : key in CSS_REST ? CSS_REST[key] : BASELINE[k];
          if (now !== undefined && val(op.from, k) !== now) {
            console.warn(`discontinuity @${op.at}: ${op.sel} ${k} from ${val(op.from, k)} but rests at ${now}`);
          }
        }
      }
      out.push(`tl.fromTo(${JSON.stringify(op.sel)}, ${JSON.stringify(op.from)}, ${JSON.stringify(to)}, ${op.at});`);
      seenSel.add(op.sel);
    } else {
      out.push(`tl.set(${JSON.stringify(op.sel)}, ${JSON.stringify(op.vars)}, ${op.at});`);
    }
    props.forEach((k) => rest.set(`${op.sel}|${k}`, val(vars, k)));
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// Beats (local seconds; global = 11.2 + t).
const FLY_DUR = 0.55;
const T3 = {
  blur: 0.3, // flight blur resolves
  tick: 0.12, // rail ticks to "See every request."
  dim: 1.15, // wall dims and defocuses
  bloom: 1.2, // mark lands at frame centre
  slide: 1.42, // mark glides aside to make room for the wordmark
  letters: 1.48, // wordmark rises letter by letter, left to right
  flip: 1.66, // rail "every" lifts off and flies into the tagline
  words: r3(1.66 + FLY_DUR - 0.08), // tagline words ripple outward as "every" lands
  pills: 2.38, // URL and licence pills pop
  sheen: 3.0, // one sheen sweep across the URL pill
};

// Lockup layout.
const LOGO = 132;
const ROW_GAP = 36;
const ROW_CY = 430;
const WORD_PX = 104;
const WORD_LH = LOGO;
const TAG_PX = 38;
const RAIL = { left: 120, top: 790, h: 80, px: 64 };
const FLY_S = TAG_PX / RAIL.px; // the rail's 80px line box scaled onto the tagline's line box
const TAG_LH = RAIL.h * FLY_S; // 47.5
const TAG_TOP = ROW_CY + LOGO / 2 + 44;
const PILL_TOP = Math.round(TAG_TOP + TAG_LH + 44);
const PILL_H = 58;
const rowW = LOGO + ROW_GAP + M.wordW;
const rowL = 960 - rowW / 2;
const D = (ROW_GAP + M.wordW) / 2; // the mark's offset from its slot while it sits at frame centre
const LOCK_CY = (ROW_CY - LOGO / 2 + PILL_TOP + PILL_H) / 2;
// Wordmark in row coordinates. The line box top sits at LOGO/2 - wordCy so the ink centres on the
// mark. Geist 500 at 104px in a 132px line box puts the baseline 103px down (ascent 105, descent
// 31, half-leading -2); the ink reaches 1.66px below it, and the mask line sits just under that.
const WORD_L = LOGO + ROW_GAP;
const WORD_TOP = LOGO / 2 - M.wordCy;
const WORD_BASE = 103;
const MASK = { l: WORD_L - 16, t: -14, w: M.wordW + 32, b: Math.ceil(WORD_TOP + WORD_BASE + 1.66 + 4) };
// Water rings from the landing point (the mark's centre while it sits at frame centre).
const RING_R0 = 58;
const RING_BOX = 640;
const RINGS = [
  { delay: 0.05, r1: 320, dur: 0.6, alpha: 0.75, w: 2 },
  { delay: 0.16, r1: 230, dur: 0.55, alpha: 0.5, w: 1.5 },
];

// ---------------------------------------------------------------------------------------------
// Optics: the flight's exit blur resolves as the cut lands, then returns softly under the dim.
const MB = '#f3-mbg';
const sd = (v) => ({ attr: { stdDeviation: v } });
ft(MB, sd('22 22'), { ...sd('0 0'), duration: T3.blur, ease: 'power3.out' }, 0);
st('#f3-optics', { filter: 'none' }, T3.blur);
st('#f3-optics', { filter: 'url(#f3-mb)' }, T3.dim);
ft(MB, sd('0 0'), { ...sd('6 6'), duration: 0.6, ease: 'power2.out' }, T3.dim);
ft('#f3-shade', { opacity: 0 }, { opacity: 1, duration: 0.6, ease: 'power2.out' }, T3.dim);

// Camera: one expo.out pull-back over the whole frame, one transform per wrapper.
const CAM_RIG = [
  ['#f3-cam', { x: 'sx', y: 'sy', z: 'zs' }],
  ['#f3-pitch', { rotationX: 'rx' }],
  ['#f3-yaw', { rotationY: 'ry' }],
  ['#f3-roll', { rotation: 'rz' }],
];
for (const [sel, map] of CAM_RIG) {
  const from = {};
  const to = {};
  for (const [prop, key] of Object.entries(map)) {
    from[prop] = CAM_FROM[key];
    to[prop] = CAM_TO[key];
  }
  ft(sel, from, { ...to, duration: DUR, ease: CAM_EASE }, 0);
}

// Wall: every tile flies from frame 0 (the cut lands mid-motion); centre ring first.
const tileTransform = (f) =>
  `translate3d(${px(f.x)},${px(f.y)},${px(f.z)}) rotate(${f.rotation}deg) rotateY(${f.rotationY}deg) rotateX(${f.rotationX}deg)`;
for (const T of TILES) {
  ft(
    `#f3-tile-${T.i}`,
    T.from,
    { x: T.to.x, y: T.to.y, z: 0, rotationX: 0, rotationY: 0, rotation: 0, duration: FLY[T.ring], ease: FLY_EASE },
    0,
  );
}
// Under the dim the wall keeps drifting, so the end card is never a freeze frame.
ft('#f3-wall', { x: 0, y: 0 }, { x: -36, y: -14, duration: r3(DUR - T3.dim), ease: 'power1.in' }, T3.dim);
// Landing flashes ripple outward ring by ring.
[0.55, 0.63, 0.71].forEach((at, ring) => {
  ft(`.f3-fr${ring}`, { opacity: 0 }, { opacity: 1, duration: 0.079, ease: 'power1.out' }, at);
  ft(`.f3-fr${ring}`, { opacity: 1 }, { opacity: 0, duration: 0.5, ease: 'power2.out' }, at + 0.08);
});

// Rail: continues from Frame 2's last line, ticks once, then hands its "every" to the tagline.
ft('#f3-rail-strip', { y: 0 }, { y: -RAIL.h, duration: 0.5, ease: 'back.out(1.4)' }, T3.tick);
ft('#f3-rail-v', { y: 0 }, { y: RAIL.h + 4, duration: 0.26, ease: 'power2.in' }, T3.flip - 0.06);
ft('#f3-rail-n', { y: 0 }, { y: RAIL.h + 4, duration: 0.26, ease: 'power2.in' }, T3.flip);
st('#f3-every-src', { opacity: 0 }, T3.flip);
st('#f3-fly', { opacity: 1 }, T3.flip);
const FLY_DX = M.tagEveryL - (RAIL.left + M.railEveryL);
const FLY_DY = TAG_TOP - RAIL.top;
// x and scale ease in-out; y leads on a sine so the word lifts first and glides in level.
ft('#f3-fly', { x: 0, scale: 1 }, { x: r2(FLY_DX), scale: r3(FLY_S), duration: FLY_DUR, ease: 'power3.inOut' }, T3.flip);
ft('#f3-fly', { y: 0 }, { y: r2(FLY_DY), duration: FLY_DUR, ease: 'sine.inOut' }, T3.flip);

// Mark: lands at centre like a drop on water (glow and two rings), then glides into the lockup.
ft('#f3-logo', { scale: 0.3, rotation: -14 }, { scale: 1, rotation: 0, duration: 0.6, ease: 'back.out(1.5)' }, T3.bloom);
ft('#f3-logo', { opacity: 0 }, { opacity: 1, duration: 0.2, ease: 'power2.out' }, T3.bloom);
ft('#f3-glow', { opacity: 0, scale: 0.8 }, { opacity: 0.38, scale: 1, duration: 0.699, ease: 'power2.out' }, T3.bloom);
ft('#f3-glow', { scale: 1 }, { scale: 1.06, duration: r3(DUR - T3.bloom - 0.7), ease: 'sine.inOut' }, T3.bloom + 0.7);
RINGS.forEach((R, k) => {
  const at = r3(T3.bloom + R.delay);
  st(`#f3-rp${k}`, { opacity: R.alpha }, at);
  ft(`#f3-rp${k}`, { attr: { r: RING_R0 } }, { attr: { r: R.r1 }, duration: R.dur, ease: 'power2.out' }, at);
  ft(`#f3-rp${k}`, { opacity: R.alpha }, { opacity: 0, duration: R.dur, ease: 'power1.out' }, at);
});
ft('#f3-row', { x: r2(D) }, { x: 0, duration: 0.62, ease: 'power3.inOut' }, T3.slide);
// Wordmark: each glyph rises through a mask line just under the baseline, in reading order.
ft('.f3-wl', { yPercent: 100 }, { yPercent: 0, duration: 0.6, ease: 'power4.out', stagger: 0.035 }, T3.letters);

// Tagline: words ripple outward from the slot "every" lands in.
ft(
  '.f3-tw',
  { y: 20, opacity: 0 },
  { y: 0, opacity: 1, duration: 0.5, ease: 'power3.out', stagger: { each: 0.045, from: 'center' } },
  T3.words,
);

// Pills: spring pop, then one sheen.
['#f3-pill-url', '#f3-pill-oss'].forEach((sel, k) => {
  const at = T3.pills + k * 0.08;
  ft(sel, { y: 16, scale: 0.7 }, { y: 0, scale: 1, duration: 0.5, ease: 'back.out(1.6)' }, at);
  ft(sel, { opacity: 0 }, { opacity: 1, duration: 0.2, ease: 'power2.out' }, at);
});
ft('#f3-sheen', { xPercent: 0 }, { xPercent: 360, duration: 0.7, ease: 'power2.inOut' }, T3.sheen);

// ---------------------------------------------------------------------------------------------
const ringOf = (T) => `f3-fr${T.ring}`;
const tileHtml = TILES.map(
  (T) =>
    `<div id="f3-tile-${T.i}" class="f3-tile" style="transform:${tileTransform(T.from)}"><div class="f3-flash ${ringOf(T)}"></div></div>`,
).join('\n                    ');
const tileCss = TILES.map(
  (T) => `    #f3-tile-${T.i} { background-image: url("../../assets/wall/${T.name}.jpg"); }`,
).join('\n');

const WORD = 'Oceans LLM';
if (M.wordXs.length !== WORD.length) throw new Error(`wordXs has ${M.wordXs.length} entries for ${WORD.length} glyphs`);
// Each letter starts below the baseline mask and rises into view, so it is clipped by design.
const wordHtml = [...WORD]
  .map((ch, k) => (ch === ' ' ? '' : `<span class="f3-wl" style="left:${px(M.wordXs[k])}" data-layout-allow-occlusion>${ch}</span>`))
  .join('');
const ringHtml = RINGS.map(
  (R, k) => `<circle id="f3-rp${k}" class="f3-rp" r="${RING_R0}" stroke-width="${R.w}" />`,
).join('\n        ');

const TAG_WORDS = ['One', 'governed', 'gateway', 'for', null, 'agent', 'and', 'every', 'model.'];
const tagHtml = TAG_WORDS.map((w) =>
  w === null
    ? `<span id="f3-tag-every" class="f3-every">every</span>`
    : `<span class="f3-tw${w === 'every' ? ' f3-every' : ''}">${w}</span>`,
).join(' ');

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
      --f3-fg: oklch(0.985 0.004 226);
      --f3-mfg: oklch(0.708 0.018 230);
      --f3-border-strong: oklch(0.985 0.004 226 / 16%);
      --f3-primary: oklch(0.513 0.112 237.173);
      --f3-wave: #0b83ad;
      --f3-mist: #b7dde4;
    }
    #f3-bg {
      position: absolute;
      inset: 0;
      background: radial-gradient(ellipse 70% 70% at 50% 45%, oklch(0.18 0.02 238) 0%, oklch(0.145 0.012 238) 55%, oklch(0.115 0.01 238) 100%);
    }
    #f3-stage {
      position: absolute;
      inset: 0;
      overflow: hidden;
    }
    #f3-fx {
      position: absolute;
      width: 0;
      height: 0;
    }
    #f3-optics {
      position: absolute;
      inset: 0;
      filter: url(#f3-mb);
    }
    #f3-lens {
      position: absolute;
      inset: 0;
      perspective: 1000px;
      perspective-origin: 50% 50%;
    }
    .f3-rig {
      position: absolute;
      inset: 0;
      transform-style: preserve-3d;
    }
    .f3-tile {
      position: absolute;
      left: 960px;
      top: 540px;
      width: ${W}px;
      height: ${H}px;
      margin-left: ${-W / 2}px;
      margin-top: ${-H / 2}px;
      border-radius: 10px;
      background-color: oklch(0.205 0.016 238);
      background-size: 100% 100%;
      box-shadow: 0 0 0 1px oklch(1 0 0 / 9%), 0 28px 56px -12px oklch(0.06 0.01 238 / 0.6);
      backface-visibility: hidden;
    }
${tileCss}
    .f3-flash {
      position: absolute;
      inset: 0;
      border-radius: inherit;
      box-shadow: inset 0 0 0 2px var(--f3-mist), inset 0 0 28px oklch(0.75 0.09 220 / 0.35);
      opacity: 0;
    }
    #f3-shade {
      position: absolute;
      inset: 0;
      background:
        radial-gradient(ellipse 1040px 500px at 50% ${r2((LOCK_CY / 1080) * 100)}%, oklch(0.1 0.01 238 / 0.66) 0%, oklch(0.1 0.01 238 / 0.34) 55%, oklch(0.1 0.01 238 / 0) 100%),
        oklch(0.115 0.012 238 / 0.36);
      opacity: 0;
    }
    /* Same scrim as Frame 2: rgba twins of oklch(0.1 0.01 238) and oklch(0.115 0.012 238) so the
       layout audit scores real alpha, and the caption pool boxed to the ellipse it paints. */
    #f3-vignette {
      position: absolute;
      inset: 0;
      background: radial-gradient(ellipse 80% 80% at 50% 45%, rgba(2, 4, 5, 0) 60%, rgba(2, 4, 5, 0.5) 100%);
    }
    #f3-pool {
      position: absolute;
      left: 0;
      bottom: 0;
      width: 1060px;
      height: 400px;
      background: radial-gradient(ellipse 1060px 400px at 0% 100%, rgba(2, 6, 8, 0.86) 0%, rgba(2, 6, 8, 0.55) 45%, rgba(2, 6, 8, 0) 100%);
    }
    #f3-rail {
      position: absolute;
      left: ${RAIL.left}px;
      top: ${RAIL.top}px;
      height: ${RAIL.h}px;
      overflow: hidden;
    }
    .f3-line {
      height: ${RAIL.h}px;
      line-height: ${RAIL.h}px;
      font-size: ${RAIL.px}px;
      letter-spacing: -0.03em;
      white-space: nowrap;
      color: var(--f3-fg);
    }
    .f3-w { display: inline-block; }
    .f3-every { color: var(--f3-wave); }
    #f3-glow {
      position: absolute;
      left: 210px;
      top: ${r2(LOCK_CY - 450)}px;
      width: 1500px;
      height: 900px;
      border-radius: 50%;
      background: radial-gradient(closest-side, oklch(0.42 0.13 240) 0%, oklch(0.36 0.12 241 / 0.55) 42%, oklch(0.3 0.1 242 / 0) 100%);
      opacity: 0;
    }
    #f3-row {
      position: absolute;
      left: ${px(rowL)};
      top: ${ROW_CY - LOGO / 2}px;
      width: ${px(rowW)};
      height: ${LOGO}px;
    }
    #f3-mark {
      position: absolute;
      left: 0;
      top: 0;
      width: ${LOGO}px;
      height: ${LOGO}px;
    }
    #f3-rings {
      position: absolute;
      left: ${960 - RING_BOX / 2}px;
      top: ${ROW_CY - RING_BOX / 2}px;
      width: ${RING_BOX}px;
      height: ${RING_BOX}px;
      overflow: visible;
    }
    .f3-rp {
      fill: none;
      stroke: var(--f3-mist);
      opacity: 0;
    }
    #f3-logo {
      position: absolute;
      inset: 0;
      width: ${LOGO}px;
      height: ${LOGO}px;
      display: block;
      opacity: 0;
    }
    #f3-word-mask {
      position: absolute;
      left: ${MASK.l}px;
      top: ${MASK.t}px;
      width: ${px(MASK.w)};
      height: ${MASK.b - MASK.t}px;
      overflow: hidden;
    }
    #f3-word {
      position: absolute;
      left: ${WORD_L - MASK.l}px;
      top: ${px(WORD_TOP - MASK.t)};
      width: ${px(M.wordW)};
      height: ${WORD_LH}px;
      font-size: ${WORD_PX}px;
      letter-spacing: -0.03em;
      white-space: nowrap;
      color: var(--f3-fg);
    }
    .f3-wl {
      position: absolute;
      top: 0;
      height: ${WORD_LH}px;
      line-height: ${WORD_LH}px;
    }
    #f3-tag {
      position: absolute;
      left: 0;
      right: 0;
      top: ${px(TAG_TOP)};
      height: ${px(TAG_LH)};
      line-height: ${px(TAG_LH)};
      font-size: ${TAG_PX}px;
      letter-spacing: -0.02em;
      text-align: center;
      white-space: nowrap;
      color: var(--f3-fg);
    }
    #f3-tag span { display: inline-block; }
    #f3-tag .f3-every { letter-spacing: -0.03em; }
    #f3-tag-every { opacity: 0; }
    #f3-fly {
      position: absolute;
      left: ${px(RAIL.left + M.railEveryL)};
      top: ${RAIL.top}px;
      height: ${RAIL.h}px;
      line-height: ${RAIL.h}px;
      font-size: ${RAIL.px}px;
      letter-spacing: -0.03em;
      white-space: nowrap;
      color: var(--f3-wave);
      transform-origin: 0 0;
      opacity: 0;
    }
    #f3-pills {
      position: absolute;
      left: 0;
      right: 0;
      top: ${PILL_TOP}px;
      height: ${PILL_H}px;
      display: flex;
      justify-content: center;
      gap: 16px;
    }
    .f3-pill {
      position: relative;
      height: ${PILL_H}px;
      padding: 0 28px;
      border-radius: ${PILL_H / 2}px;
      display: flex;
      align-items: center;
      gap: 12px;
      white-space: nowrap;
      opacity: 0;
    }
    #f3-pill-url {
      overflow: hidden;
      background: var(--f3-primary);
      color: var(--f3-fg);
      font-family: "JetBrains Mono", monospace;
      font-variant-ligatures: none;
      font-weight: 400;
      font-size: 26px;
    }
    #f3-sheen {
      position: absolute;
      top: 0;
      bottom: 0;
      left: -40%;
      width: 40%;
      background: linear-gradient(100deg, oklch(1 0 0 / 0) 0%, oklch(1 0 0 / 0.3) 50%, oklch(1 0 0 / 0) 100%);
    }
    #f3-pill-oss {
      background: oklch(0.165 0.014 238 / 0.72);
      box-shadow: inset 0 0 0 1.5px var(--f3-border-strong);
      color: var(--f3-fg);
      font-size: 24px;
    }
    .f3-gh { width: 24px; height: 24px; color: var(--f3-fg); }
    .f3-dot, .f3-lic { color: var(--f3-mfg); }
    .f3-lic { font-family: "JetBrains Mono", monospace; font-variant-ligatures: none; font-weight: 400; font-size: 23px; }
  </style>

  <div id="root" data-composition-id="f3-lockup" data-width="1920" data-height="1080">
    <div id="f3-bg" class="clip" data-start="0" data-duration="${DUR}" data-track-index="0"></div>

    <div id="f3-stage" class="clip" data-start="0" data-duration="${DUR}" data-track-index="1">
      <svg id="f3-fx" width="0" height="0" aria-hidden="true" focusable="false">
        <filter id="f3-mb" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB">
          <feGaussianBlur id="f3-mbg" stdDeviation="22 22" />
        </filter>
      </svg>
      <div id="f3-optics">
        <div id="f3-lens">
          <div id="f3-cam" class="f3-rig" style="transform:translate3d(${px(CAM_FROM.sx)},${px(CAM_FROM.sy)},${px(CAM_FROM.zs)})">
            <div id="f3-pitch" class="f3-rig" style="transform:rotateX(${CAM_FROM.rx}deg)">
              <div id="f3-yaw" class="f3-rig" style="transform:rotateY(${CAM_FROM.ry}deg)">
                <div id="f3-roll" class="f3-rig" style="transform:rotate(${CAM_FROM.rz}deg)">
                  <div id="f3-wall" class="f3-rig" data-layout-allow-overflow>
                    ${tileHtml}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div id="f3-shade"></div>
    </div>

    <div id="f3-hud" class="clip" data-start="0" data-duration="${DUR}" data-track-index="2">
      <div id="f3-vignette"></div>
      <div id="f3-pool"></div>
      <div id="f3-rail">
        <div id="f3-rail-strip" data-layout-allow-overflow>
          <div class="f3-line" data-layout-allow-occlusion>Share <span class="f3-every" data-layout-allow-occlusion>every</span> skill.</div>
          <div class="f3-line"><span id="f3-rail-v" class="f3-w" data-layout-allow-occlusion>See</span> <span id="f3-every-src" class="f3-w f3-every">every</span> <span id="f3-rail-n" class="f3-w" data-layout-allow-occlusion>request.</span></div>
        </div>
      </div>
    </div>

    <div id="f3-lockup" class="clip" data-start="0" data-duration="${DUR}" data-track-index="3">
      <div id="f3-glow"></div>
      <svg id="f3-rings" viewBox="${-RING_BOX / 2} ${-RING_BOX / 2} ${RING_BOX} ${RING_BOX}" aria-hidden="true" focusable="false">
        ${ringHtml}
      </svg>
      <div id="f3-row">
        <div id="f3-mark">
          <img id="f3-logo" src="../../assets/brand/oceans-icon.png" alt="Oceans" />
        </div>
        <div id="f3-word-mask"><div id="f3-word" aria-label="Oceans LLM" data-layout-allow-overflow>${wordHtml}</div></div>
      </div>
      <div id="f3-tag">${tagHtml}</div>
      <div id="f3-fly">every</div>
      <div id="f3-pills">
        <div id="f3-pill-url" class="f3-pill"><div id="f3-sheen" data-layout-allow-overflow></div>oceans-llm.com</div>
        <div id="f3-pill-oss" class="f3-pill"><svg class="f3-gh" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd" aria-hidden="true">${GITHUB}</svg><span>Open source</span><span class="f3-dot">·</span><span class="f3-lic">AGPL-3.0</span></div>
      </div>
    </div>
  </div>

  <script>
    (function () {
      window.__timelines = window.__timelines || {};
      var tl = gsap.timeline({ paused: true });

      ${emitOps().join('\n      ')}

      tl.set({}, {}, ${DUR});
      window.__timelines['f3-lockup'] = tl;
    })();
  </script>
</template>
`;

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, html);
console.log(`wrote ${path.relative(ROOT, OUT)} (${html.length} bytes, ${OPS.length} timeline ops)`);
console.log(
  `layout: row ${r2(rowL)}..${r2(rowL + rowW)}  slide D ${r2(D)}  fly d(${r2(FLY_DX)}, ${r2(FLY_DY)}) s ${r3(FLY_S)}  tag top ${TAG_TOP}  pills top ${PILL_TOP}  lockup cy ${r2(LOCK_CY)}  word mask y ${MASK.t}..${MASK.b}`,
);
