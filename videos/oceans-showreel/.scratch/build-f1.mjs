#!/usr/bin/env node
// Builds compositions/frames/f1-converge.html — Frame 1, "Converge".
// Agents snap onto a left arc, providers onto a right arc, twelve wires carry
// pulses into the Oceans tile, then the camera punches into its deep wave band.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ICON_DIR = path.resolve(
  ROOT,
  '../../crates/admin-ui/web/node_modules/@lobehub/icons-static-svg/icons',
);
const OUT = path.join(ROOT, 'compositions/frames/f1-converge.html');

const lobe = (name) =>
  fs
    .readFileSync(path.join(ICON_DIR, `${name}.svg`), 'utf8')
    .replace(/^[\s\S]*?<\/title>/, '')
    .replace(/<\/svg>\s*$/, '')
    .trim();
const svg24 = (inner) =>
  `<svg class="f1-ico" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd" aria-hidden="true">${inner}</svg>`;
const PI_MARK = `<svg class="f1-ico" viewBox="0 0 64 64" fill="currentColor" aria-hidden="true"><path d="M10 14h44v9H43v33h-9V23h-9v22h-9V23H10z"/></svg>`;

const AGENTS = [
  ['Claude Code', svg24(lobe('claudecode'))],
  ['Codex', svg24(lobe('codex'))],
  ['OpenCode', svg24(lobe('opencode'))],
  ['Gemini CLI', svg24(lobe('geminicli'))],
  ['GitHub Copilot', svg24(lobe('githubcopilot'))],
  ['Pi', PI_MARK],
];
const PROVIDERS = [
  ['Anthropic', 'anthropic'],
  ['OpenAI', 'openai'],
  ['Vertex AI', 'vertexai'],
  ['Bedrock', 'bedrock'],
  ['OpenRouter', 'openrouter'],
  ['DeepSeek', 'deepseek'],
].map(([name, icon]) => [name, svg24(lobe(icon))]);

// Geometry (1920×1080). Hub centred; arcs bulge outward like ( ).
const CX = 960;
const CY = 540;
const R = 640;
const TILE = 84;
const HUB = 220;
const ANGLES = [-27, -16.2, -5.4, 5.4, 16.2, 27];
const NOUN_H = 138;
const r1 = (v) => Math.round(v * 10) / 10;
const rad = (d) => (d * Math.PI) / 180;

const arc = (side) =>
  ANGLES.map((a) => ({
    x: CX + side * R * Math.cos(rad(a)),
    y: CY + R * Math.sin(rad(a)),
    rank: [2, 1, 0, 0, 1, 2][ANGLES.indexOf(a)],
  }));

function tiles(list, side, cls) {
  return arc(side)
    .map((p, i) => {
      const [name, ico] = list[i];
      const labelW = 240;
      const gap = 20;
      const lx = side < 0 ? p.x - TILE / 2 - gap - labelW : p.x + TILE / 2 + gap;
      return [
        `<div class="f1-tile ${cls}" style="left:${r1(p.x - TILE / 2)}px;top:${r1(p.y - TILE / 2)}px">${ico}</div>`,
        `<div class="f1-label ${cls}-label" style="left:${r1(lx)}px;top:${r1(p.y - 14)}px;text-align:${side < 0 ? 'right' : 'left'}">${name}</div>`,
      ].join('\n        ');
    })
    .join('\n        ');
}

function wirePath(p, side) {
  const x0 = p.x - side * (TILE / 2 + 10);
  const y0 = p.y;
  const x3 = CX + side * (HUB / 2 + 6);
  const y3 = CY + (p.y - CY) * 0.16;
  const dx = x3 - x0;
  return `M${r1(x0)} ${r1(y0)}C${r1(x0 + dx * 0.55)} ${r1(y0)} ${r1(x3 - dx * 0.5)} ${r1(y3)} ${r1(x3)} ${r1(y3)}`;
}

const wireSet = [...arc(-1).map((p) => [p, -1]), ...arc(1).map((p) => [p, 1])];
const wires = wireSet
  .map(
    ([p, side]) =>
      `<path class="f1-wire" data-rank="${p.rank}" pathLength="1000" d="${wirePath(p, side)}"/>`,
  )
  .join('\n          ');
const comets = wireSet
  .map(
    ([p, side]) =>
      `<path class="f1-tail" data-rank="${p.rank}" pathLength="1000" d="${wirePath(p, side)}"/>` +
      `<path class="f1-spark" data-rank="${p.rank}" pathLength="1000" d="${wirePath(p, side)}"/>`,
  )
  .join('\n          ');

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
    #root {
      position: absolute;
      inset: 0;
      width: 1920px;
      height: 1080px;
      overflow: hidden;
      font-family: "Geist", sans-serif;
      font-weight: 500;
      color: oklch(0.985 0 0);
    }
    #f1-bg {
      position: absolute;
      inset: 0;
      background:
        radial-gradient(ellipse 60% 62% at 50% 50%, oklch(0.18 0.02 238) 0%, oklch(0.145 0.012 238) 62%, oklch(0.12 0.01 238) 100%);
    }
    #f1-deep {
      position: absolute;
      inset: 0;
      background: #0a6192;
      opacity: 0;
    }
    #f1-scene {
      position: absolute;
      inset: 0;
      overflow: hidden;
    }
    #f1-world {
      position: absolute;
      inset: 0;
      transform-origin: 956px 608px;
    }
    #f1-glow {
      position: absolute;
      left: 440px;
      top: 20px;
      width: 1040px;
      height: 1040px;
      border-radius: 50%;
      background: radial-gradient(closest-side, oklch(0.42 0.13 240) 0%, oklch(0.36 0.12 241 / 0.55) 42%, oklch(0.3 0.1 242 / 0) 100%);
      opacity: 0;
    }
    #f1-orbit {
      position: absolute;
      inset: 0;
    }
    #f1-wires {
      position: absolute;
      left: 0;
      top: 0;
      width: 1920px;
      height: 1080px;
      overflow: visible;
    }
    .f1-wire {
      fill: none;
      stroke: #0b83ad;
      stroke-opacity: 0.5;
      stroke-width: 1.5;
      stroke-dasharray: 1000;
      stroke-dashoffset: 1000;
    }
    .f1-tail,
    .f1-spark {
      fill: none;
      opacity: 0;
    }
    .f1-tail {
      stroke: #0b83ad;
      stroke-width: 3;
      stroke-linecap: butt;
      stroke-dasharray: 160 2000;
      stroke-dashoffset: 160;
    }
    .f1-spark {
      stroke: oklch(0.985 0 0);
      stroke-width: 3.5;
      stroke-linecap: round;
      stroke-dasharray: 35 2000;
      stroke-dashoffset: 35;
    }
    .f1-tile {
      position: absolute;
      width: ${TILE}px;
      height: ${TILE}px;
      border-radius: 18px;
      background: oklch(0.205 0 0);
      border: 1px solid oklch(1 0 0 / 10%);
      box-shadow: 0 18px 40px oklch(0 0 0 / 0.35);
      display: flex;
      align-items: center;
      justify-content: center;
      color: oklch(0.985 0 0);
    }
    .f1-ico {
      width: 40px;
      height: 40px;
      display: block;
    }
    .f1-label {
      position: absolute;
      width: 240px;
      height: 28px;
      line-height: 28px;
      font-size: 20px;
      color: oklch(0.708 0 0);
      white-space: nowrap;
    }
    #f1-ripple {
      position: absolute;
      left: ${CX - HUB / 2}px;
      top: ${CY - HUB / 2}px;
      width: ${HUB}px;
      height: ${HUB}px;
      border-radius: 50px;
      border: 2px solid #0b83ad;
      opacity: 0;
    }
    #f1-hub {
      position: absolute;
      left: ${CX - HUB / 2}px;
      top: ${CY - HUB / 2}px;
      width: ${HUB}px;
      height: ${HUB}px;
      display: block;
    }
    #f1-head {
      position: absolute;
      left: 0;
      right: 0;
      top: ${CY - NOUN_H / 2}px;
      height: ${NOUN_H}px;
      display: flex;
      justify-content: center;
      align-items: flex-start;
      gap: 0.26em;
      font-size: 120px;
      letter-spacing: -0.035em;
      line-height: ${NOUN_H}px;
      white-space: nowrap;
    }
    #f1-every {
      color: #0b83ad;
    }
    #f1-noun-mask {
      display: inline-block;
      height: ${NOUN_H}px;
      overflow: hidden;
    }
    #f1-noun-strip {
      display: flex;
      flex-direction: column;
    }
    .f1-noun {
      display: block;
      height: ${NOUN_H}px;
      line-height: ${NOUN_H}px;
    }
    #f1-tag {
      position: absolute;
      left: 0;
      right: 0;
      top: 690px;
      height: 100px;
      line-height: 100px;
      text-align: center;
      font-size: 84px;
      letter-spacing: -0.03em;
      white-space: nowrap;
    }
    #f1-veil-fill {
      position: absolute;
      inset: 0;
      background: #0a6192;
      opacity: 0;
    }
  </style>

  <div id="root" data-composition-id="f1-converge" data-width="1920" data-height="1080">
    <div id="f1-bg" class="clip" data-start="0" data-duration="2.8" data-track-index="0"><div id="f1-deep"></div></div>

    <div id="f1-scene" class="clip" data-start="0" data-duration="2.8" data-track-index="1">
      <div id="f1-world" data-layout-allow-overflow>
        <div id="f1-glow"></div>
        <div id="f1-orbit">
          <svg id="f1-wires" viewBox="0 0 1920 1080" aria-hidden="true">
          ${wires}
          ${comets}
          </svg>
        ${tiles(AGENTS, -1, 'f1-agent')}
        ${tiles(PROVIDERS, 1, 'f1-provider')}
        </div>
        <div id="f1-ripple"></div>
        <img id="f1-hub" src="../../assets/brand/oceans-icon.png" alt="Oceans" />
        <div id="f1-head">
          <span id="f1-every">Every</span>
          <span id="f1-noun-mask"><span id="f1-noun-strip"><span class="f1-noun">agent.</span><span class="f1-noun">model.</span></span></span>
        </div>
        <div id="f1-tag">One gateway.</div>
      </div>
    </div>

    <div id="f1-veil" class="clip" data-start="0" data-duration="2.8" data-track-index="2"><div id="f1-veil-fill"></div></div>
  </div>

  <script>
    (function () {
      var NOUN_H = ${NOUN_H};
      window.__timelines = window.__timelines || {};
      var tl = gsap.timeline({ paused: true });

      // Beat 1 (0.08s): "Every agent." slams in; the agents snap onto the left arc.
      tl.fromTo('#f1-head',
        { opacity: 0, scale: 1.5, filter: 'blur(16px)' },
        { opacity: 1, scale: 1, filter: 'blur(0px)', duration: 0.5, ease: 'power4.out' }, 0.08);
      tl.fromTo('.f1-agent',
        { opacity: 0, x: -64, scale: 0.8, filter: 'blur(6px)' },
        { opacity: 1, x: 0, scale: 1, filter: 'blur(0px)', duration: 0.55, ease: 'power3.out', stagger: 0.05 }, 0.18);
      tl.fromTo('.f1-agent-label',
        { opacity: 0, x: -18 },
        { opacity: 1, x: 0, duration: 0.4, ease: 'power3.out', stagger: 0.05 }, 0.26);

      // Beat 2 (0.9s): the noun ticks to "model."; the providers land on the right arc.
      tl.fromTo('#f1-noun-strip', { y: 0 }, { y: -NOUN_H, duration: 0.5, ease: 'back.out(1.4)' }, 0.9);
      tl.fromTo('.f1-provider',
        { opacity: 0, x: 64, scale: 0.8, filter: 'blur(6px)' },
        { opacity: 1, x: 0, scale: 1, filter: 'blur(0px)', duration: 0.55, ease: 'power3.out', stagger: 0.05 }, 0.94);
      tl.fromTo('.f1-provider-label',
        { opacity: 0, x: 18 },
        { opacity: 1, x: 0, duration: 0.4, ease: 'power3.out', stagger: 0.05 }, 1.02);

      // Beat 3 (1.32s): the type clears, twelve wires draw inward, pulses race to the centre.
      tl.fromTo('#f1-head',
        { opacity: 1, scale: 1, filter: 'blur(0px)' },
        { opacity: 0, scale: 0.9, filter: 'blur(10px)', duration: 0.24, ease: 'power2.in', immediateRender: false }, 1.32);
      // Paths use pathLength=1000: GSAP rounds px values to whole units, so a pathLength of 1
      // would snap. Wires draw from the outer pair inward. Each pulse's spark and tail share a
      // leading edge (offset = dash length - travel) and travel 1160 so the tail fully drains.
      document.querySelectorAll('.f1-wire').forEach(function (el) {
        var at = 1.36 + (2 - Number(el.getAttribute('data-rank'))) * 0.016;
        tl.fromTo(el, { strokeDashoffset: 1000 }, { strokeDashoffset: 0, duration: 0.32, ease: 'power2.inOut' }, at);
      });
      document.querySelectorAll('.f1-tail, .f1-spark').forEach(function (el) {
        var at = 1.42 + (2 - Number(el.getAttribute('data-rank'))) * 0.012;
        var len = el.classList.contains('f1-tail') ? 160 : 35;
        tl.set(el, { opacity: 1 }, at);
        tl.fromTo(el, { strokeDashoffset: len }, { strokeDashoffset: len - 1160, duration: 0.34, ease: 'power2.in' }, at);
        tl.set(el, { opacity: 0 }, at + 0.34);
      });

      // Beat 4 (1.74s): the Oceans tile springs up with a bloom and a ripple, then "One gateway."
      tl.fromTo('#f1-hub', { opacity: 0, scale: 0.55 }, { opacity: 1, scale: 1, duration: 0.6, ease: 'expo.out' }, 1.74);
      tl.fromTo('#f1-glow', { opacity: 0, scale: 0.7 }, { opacity: 0.38, scale: 1, duration: 0.6, ease: 'power2.out' }, 1.74);
      tl.fromTo('#f1-glow', { scale: 1 }, { scale: 1.04, duration: 0.46, ease: 'sine.inOut', immediateRender: false }, 2.34);
      tl.fromTo('#f1-ripple', { opacity: 0, scale: 0.96 }, { opacity: 0.85, scale: 1, duration: 0.05, ease: 'none' }, 1.76);
      tl.to('#f1-ripple', { opacity: 0, scale: 1.9, duration: 0.72, ease: 'power2.out' }, 1.81);
      tl.fromTo('#f1-tag',
        { opacity: 0, y: 50, filter: 'blur(12px)' },
        { opacity: 1, y: 0, filter: 'blur(0px)', duration: 0.5, ease: 'circ.out' }, 1.86);

      // Beat 5 (2.16s): focus pulls to the hub, then the camera punches into the deep wave band.
      tl.fromTo('#f1-orbit',
        { opacity: 1, filter: 'blur(0px)' },
        { opacity: 0.55, filter: 'blur(4px)', duration: 0.3, ease: 'power2.inOut' }, 2.16);
      tl.fromTo('#f1-world', { scale: 1 }, { scale: 1.035, duration: 2.45, ease: 'power1.in' }, 0);
      tl.fromTo('#f1-world', { scale: 1.035 }, { scale: 22, duration: 0.33, ease: 'power3.in', immediateRender: false }, 2.45);
      tl.fromTo('#f1-scene', { filter: 'blur(0px)' }, { filter: 'blur(20px)', duration: 0.33, ease: 'power3.in' }, 2.45);
      // Deep-blue underlay so the blurred scene's edges never fringe dark during the punch.
      tl.fromTo('#f1-deep', { opacity: 0 }, { opacity: 1, duration: 0.2, ease: 'power2.in' }, 2.48);
      tl.fromTo('#f1-veil-fill', { opacity: 0 }, { opacity: 1, duration: 0.18, ease: 'power2.in' }, 2.56);

      tl.set({}, {}, 2.8);
      window.__timelines['f1-converge'] = tl;
    })();
  </script>
</template>
`;

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, html);
console.log(`wrote ${path.relative(ROOT, OUT)} (${html.length} bytes)`);
