#!/usr/bin/env node
// Builds assets/audio/score.wav: the showreel's original score, synthesized offline and cut to
// the picture. 150 BPM (beat 0.4s, bar 1.6s), so the structural cuts land on downbeats: the
// match cut into the flight (2.8s), the exit push (10.8s) and the logo (12.4s).
//
//   0.00–2.80  Converge: free-time slams on Dm and B♭, glass plucks as the tiles spring in, a zip
//              on the pulses, the hub's water drop, then a snare-roll build over A and a breath.
//   2.80–11.2  Flight: a half-time groove (Dm B♭ F C Dm B♭) with in-key accents on every proof.
//   10.8–11.2  The exit push pulls the whole groove underwater (a closing low-pass).
//   11.2–12.4  The wall assembles: Csus surfaces through the opening filter under a riser.
//   12.4–15.0  The mark drops onto D major, the rings ring, a harp run, a bell ripple, fade.
//
// Deterministic: every noise source is seeded, so re-running reproduces the file exactly.
// Usage: node scripts/score.mjs (npm run dev, check, render and publish run it first; the WAV is gitignored).
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  SR, Bus, SVF, hz, kick, snare, hat, crash, reverseCymbal, boom, sub, bassPluck, pad, pluck, bell,
  drop, riser, blip, beep, buzz, freeverb, pingpong, ducker, lufs, limit, peakDb, writeWav,
} from './score-dsp.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT = path.join(ROOT, 'assets/audio/score.wav');

const LEN = 15;
const S16 = 0.1; // one 16th at 150 BPM
const DROP = 2.8; // match cut into the flight
const STOP = 11.2; // cut to the lockup; the groove stops
const LOGO = 12.4; // the mark lands
const BREATHS = [
  [2.755, DROP],
  [12.355, LOGO],
]; // near-silence right before each big landing
const TARGET_LUFS = -15; // the SFX layer adds roughly 1 LU on top
const CEILING = 0.79; // −2 dBFS sample peak: room for inter-sample peaks and the SFX
const FX_GAIN = 1.4;
const REV_RETURN = 1.6;
const DLY_RETURN = 0.6;

const bus = () => new Bus(LEN);
const drums = bus();
const reese = bus(); // rolling 16th bass, high-passed; the sub carries the fundamental
const subs = bus();
const keys = bus(); // groove pads, sidechained
const pads = bus(); // free-time stabs, the build and the final chord, not sidechained
const arp = bus();
const fx = bus(); // in-key UI accents
const air = bus(); // risers and reverse cymbals
const rev = bus(); // reverb send
const dly = bus(); // ping-pong delay send
const kicks = []; // sidechain triggers

// Place a mono or stereo source on a bus at time t, with optional reverb and delay sends.
function put(target, src, t, gain = 1, pan = 0, send = {}) {
  const add = (b, g) => (src.l ? b.stereo(src.l, src.r, t, g) : b.mono(src, t, g, pan));
  add(target, gain);
  if (send.rev) add(rev, gain * send.rev);
  if (send.dly) add(dly, gain * send.dly);
}

const KICK = kick({ f0: 150, f1: 48, decay: 0.2 });
const KICK_BIG = kick({ f0: 170, f1: 44, decay: 0.34, len: 0.8, click: 0.55, drive: 2.2, seed: 11 });
const KICK_SOFT = kick({ f0: 120, f1: 52, decay: 0.16, len: 0.35, click: 0.2, seed: 12 });
const SNARE = snare();
const ROLL = snare({ decay: 0.06, len: 0.12, body: 0.35, seed: 13 });
const ROLL_CUT = snare({ decay: 0.02, len: 0.045, body: 0.35, seed: 14 }); // ends inside the breath
const HATS = [3, 4, 5, 6].map((seed) => hat({ seed }));
const OPEN_HAT = hat({ decay: 0.14, len: 0.5, seed: 15 });
const CRASH = crash();
const CRASH_BIG = crash({ len: 3.0, decay: 1.4, seed: 16 });

function hitKick(t, src = KICK, gain = 0.85) {
  put(drums, src, t, gain);
  kicks.push(t);
}

// Voicings: pad (supersaw), bass root (sub + reese), arp pool (5 notes, low to high).
const CHORD = {
  Dm: { pad: ['D3', 'A3', 'D4', 'F4', 'A4', 'E5'], root: 'D2', arp: ['D4', 'F4', 'A4', 'D5', 'E5'] },
  Bb: { pad: ['Bb2', 'F3', 'D4', 'F4', 'A4', 'D5'], root: 'Bb1', arp: ['D4', 'F4', 'A4', 'Bb4', 'D5'] },
  F: { pad: ['F3', 'C4', 'F4', 'A4', 'C5', 'G5'], root: 'F2', arp: ['F4', 'A4', 'C5', 'F5', 'G5'] },
  C: { pad: ['C3', 'G3', 'E4', 'G4', 'C5', 'D5'], root: 'C2', arp: ['E4', 'G4', 'C5', 'D5', 'E5'] },
  Csus: { pad: ['C3', 'G3', 'C4', 'D4', 'G4', 'D5'], root: 'C2', arp: ['C4', 'D4', 'G4', 'C5', 'D5'] },
  D: { pad: ['D3', 'A3', 'D4', 'F#4', 'A4', 'E5', 'F#5'], root: 'D2' },
};
// Flight bars: [downbeat, chord, 16ths]. The last bar is one beat long: the exit push.
const BARS = [
  [2.8, 'Dm', 16],
  [4.4, 'Bb', 16],
  [6.0, 'F', 16],
  [7.6, 'C', 16],
  [9.2, 'Dm', 16],
  [10.8, 'Bb', 4],
];
const KICK_STEPS = new Set([0, 6, 10]);
const ARP_SHAPE = [0, 1, 2, 3, 4, 3, 2, 1, 0, 2, 4, 2, 3, 1, 4, 2];
const HAT_VEL = [0.5, 0.22, 0.75, 0.3];

// 0–2.8s: the cold open and the build into the match cut.
function converge() {
  // Two free-time slams: "Every agent." on Dm, "model." on B♭.
  const slam = (t, notes, root, g, seed) => {
    hitKick(t, KICK_BIG, 0.95 * g);
    put(drums, boom({ f0: 140, f1: hz(root), decay: 0.6, len: 1.8, seed }), t, 0.42 * g);
    put(drums, SNARE, t, 0.3 * g, 0, { rev: 0.7 });
    put(drums, CRASH, t, 0.2 * g, 0, { rev: 0.3 });
    const cutoff = (x) => 280 + 4800 * Math.exp(-x / 0.08);
    put(pads, pad({ notes, len: 0.1, attack: 0.002, release: 0.3, voices: 5, detune: 16, cutoff, q: 0.9, seed }), t, 0.5 * g, 0, { rev: 0.7 });
  };
  slam(0.08, ['D2', 'A2', 'D3', 'F3', 'A3', 'D4'], 'D2', 1, 21);
  slam(0.9, ['Bb1', 'F2', 'Bb2', 'D3', 'F3', 'Bb3'], 'Bb1', 0.85, 22);
  // A low drone under the slams, so the cold open never drops to silence.
  const drone = (t, len, notes, fc, seed) =>
    put(pads, pad({ notes, len, attack: 0.05, release: 0.1, voices: 5, cutoff: () => fc, seed }), t, 0.28, 0, { rev: 0.3 });
  drone(0.08, 0.82, ['D2', 'A2', 'D3', 'F3', 'A3'], 650, 23);
  drone(0.9, 0.84, ['Bb1', 'F2', 'Bb2', 'D3', 'F3', 'A3'], 700, 24);
  drone(1.74, 0.28, ['D2', 'A2', 'D3', 'E3', 'F3', 'A3'], 900, 25);
  // Glass plucks as the tiles spring in: agents on the left arc, providers on the right.
  const glass = (n) => bell({ f: hz(n), ratio: 2, index: 1.3, decay: 0.32, idecay: 0.06, glass: 0.35, len: 1.2 });
  ['A4', 'D5', 'F5', 'A5', 'D6', 'F6'].forEach((n, k) => put(fx, glass(n), 0.18 + k * 0.05, 0.15, -0.75 + k * 0.06, { rev: 0.45 }));
  ['Bb4', 'D5', 'F5', 'A5', 'Bb5', 'D6'].forEach((n, k) => put(fx, glass(n), 0.94 + k * 0.05, 0.15, 0.75 - k * 0.06, { rev: 0.45 }));
  // The pulses race inward (power2.in): a zip that lands on the hub, which drops in like water.
  put(air, riser({ len: 0.34, f0: 1200, f1: 9000, from: 'D4', to: 'D6', tone: 0.45, seed: 26 }), 1.41, 0.3);
  hitKick(1.74, KICK_SOFT, 0.7);
  put(fx, drop({ f0: 520, f1: hz('D6') }), 1.74, 0.3, 0, { rev: 0.5 });
  put(fx, bell({ f: hz('D6'), ratio: 3.5, index: 2.0, decay: 1.1 }), 1.75, 0.24, 0, { rev: 0.5 });
  put(fx, bell({ f: hz('A6'), ratio: 3.5, index: 1.5, decay: 0.8 }), 1.78, 0.11, 0.2, { rev: 0.5 });
  // Build over A: a snare roll (16ths, then 32nds), quarter kicks, an opening pad, a reverse
  // cymbal and a riser, all ending 45ms early so the drop lands out of a breath.
  for (let k = 0; k < 4; k++) put(drums, ROLL, 2.0 + k * 0.1, 0.24 + k * 0.05, 0, { rev: 0.25 });
  for (let k = 0; k < 7; k++) put(drums, k === 6 ? ROLL_CUT : ROLL, 2.4 + k * 0.05, 0.4 + k * 0.07, 0, { rev: 0.25 });
  hitKick(2.0, KICK, 0.5);
  hitKick(2.4, KICK, 0.6);
  const open = (x) => 500 * 12 ** Math.min(1, (x / 0.72) ** 2);
  put(pads, pad({ notes: ['A1', 'E2', 'A2', 'D3', 'E3', 'A3'], len: 0.715, attack: 0.5, release: 0.008, cutoff: open, seed: 27 }), 2.04, 0.42, 0, { rev: 0.2 });
  put(air, reverseCymbal({ len: 0.9, seed: 28 }), BREATHS[0][0] - 0.9, 0.55);
  put(air, riser({ len: 0.8, f0: 350, f1: 8000, from: 'A2', to: 'A4', tone: 0.3, seed: 29 }), BREATHS[0][0] - 0.8, 0.5);
}

// 2.8–11.2s: the half-time groove under the flight.
function groove() {
  BARS.forEach(([t0, name, steps], b) => {
    const C = CHORD[name];
    const len = steps * S16;
    const first = b === 0;
    // The first bar's pad blooms open with the dive; later bars sit back behind the arp.
    const cutoff = first ? (x) => 1300 * (3200 / 1300) ** Math.min(1, x / 0.7) : () => 2600;
    put(keys, pad({ notes: C.pad, len, attack: first ? 0.004 : 0.03, release: 0.06, cutoff, seed: 40 + b }), t0, 0.38, 0, { rev: 0.18 });
    put(subs, sub({ f: hz(C.root), len, attack: 0.004, release: 0.04 }), t0, 0.32);
    for (let s = 0; s < steps; s++) {
      const t = t0 + s * S16;
      if (KICK_STEPS.has(s)) hitKick(t, first && s === 0 ? KICK_BIG : KICK, first && s === 0 ? 1 : 0.85);
      else put(reese, bassPluck({ f: hz(C.root) * (s % 4 === 3 ? 2 : 1), seed: 50 + s }), t, s % 4 === 2 ? 0.34 : 0.27);
      if (s === 8) put(drums, SNARE, t, 0.62, 0, { rev: 0.22 });
      put(drums, HATS[s % 4], t, 0.2 * HAT_VEL[s % 4], 0.18);
      const bright = first ? 1800 + 3000 * (s / 16) : 4800;
      put(arp, pluck({ f: hz(C.arp[ARP_SHAPE[s]]), bright, seed: 60 + s }), t, s % 2 ? 0.24 : 0.32, 0, { dly: 0.4, rev: 0.12 });
    }
  });
  // Crashes on the section downbeats, biggest on the drop and after the dive fill.
  for (const [t, g] of [
    [2.8, 0.55],
    [4.4, 0.28],
    [6.0, 0.24],
    [7.6, 0.28],
    [9.2, 0.45],
    [10.8, 0.32],
  ])
    put(drums, CRASH, t, g, 0, { rev: 0.12 });
  put(drums, boom({ f0: 140, f1: hz('D1'), decay: 0.6, seed: 30 }), DROP, 0.4);
  // Open hats lift the back half of the B♭ and C bars; a 16th fill rides the dive into stop 4.
  for (const t0 of [4.4, 7.6]) put(drums, OPEN_HAT, t0 + 14 * S16, 0.16, 0.2);
  for (let k = 0; k < 4; k++) put(drums, ROLL, 8.8 + k * 0.1, 0.25 + k * 0.1, 0, { rev: 0.2 });
}

// In-key accents on each proof the flight lands on (global times = Frame 2 local + 2.8).
function accents() {
  const tick = (n, t, g, pan = 0.1) =>
    put(fx, bell({ f: hz(n), ratio: 2, index: 0.9, decay: 0.09, idecay: 0.03, glass: 0.2, len: 0.4 }), t, g, pan, { rev: 0.25 });
  // Stop 1, routing: the rows pop in, the weighted pick hops twice, then confirms with a ping.
  [3.4, 3.46, 3.52].forEach((t, k) => tick(['A5', 'D6', 'F6'][k], t, 0.07));
  for (const [t, n] of [
    [3.92, 'D6'],
    [4.1, 'A5'],
    [4.28, 'D6'],
  ])
    tick(n, t, 0.2);
  put(fx, bell({ f: hz('D6'), ratio: 3.5, index: 1.6, decay: 0.7 }), 4.42, 0.22, 0.1, { rev: 0.4 });
  put(fx, bell({ f: hz('F6'), ratio: 3.5, index: 1.2, decay: 0.6 }), 4.44, 0.12, 0.1, { rev: 0.4 });
  // Stop 2, spend: the counter ratchets up its power2.out curve in $12 steps, and "Near limit"
  // lands as a double beep on the frame the fill crosses $120 (80% of the $150 budget).
  const AMT_AT = 5.35;
  const AMT_DUR = 0.85;
  const AMT_END = 132;
  const crossAt = (v) => AMT_AT + AMT_DUR * (1 - Math.cbrt(1 - v / AMT_END));
  for (let v = 12; v < 120; v += 12) put(fx, blip({ f: 1500 + 1500 * (v / AMT_END) }), crossAt(v), 0.06, 0.1);
  for (const t of [crossAt(120), crossAt(120) + 0.1]) put(fx, beep({ f: hz('F6') }), t, 0.12, 0.1, { rev: 0.2 });
  // Stop 3, guardrails: the decision pills confirm on C.
  for (const [t, n] of [
    [8.1, 'C6'],
    [8.16, 'E6'],
    [8.22, 'G6'],
  ])
    tick(n, t, 0.15);
  // Stop 4, tool sets: the dropped chips sink, the four kept tools tick into the set (the count
  // goes 1–4 up D minor), and github.delete_repository is denied with the shake's 9 Hz wobble.
  put(fx, drop({ f0: 900, f1: 320, sweep: 0.45, decay: 0.22, len: 0.9 }), 9.32, 0.07, 0, { rev: 0.4 });
  for (const [t, n] of [
    [9.54, 'D5'],
    [9.61, 'F5'],
    [9.68, 'A5'],
    [9.75, 'D6'],
  ])
    put(fx, pluck({ f: hz(n), decay: 0.12, bright: 3600 }), t, 0.2, 0.15, { rev: 0.25 });
  put(fx, buzz({ f: hz('D3'), len: 0.42, wobble: 9.1 }), 10.0, 0.3, 0.2, { rev: 0.2 });
}

// 11.2–12.4s: the wall assembles out of the blur while the music surfaces.
function surface() {
  // The cut: a soft sub drop as the groove stops, still underwater.
  put(drums, boom({ f0: 95, f1: hz('C2'), decay: 0.5, thud: 0.3, seed: 31 }), STOP, 0.35);
  const C = CHORD.Csus;
  const end = BREATHS[1][0];
  put(keys, pad({ notes: C.pad, len: end - STOP, attack: 0.05, release: 0.006, cutoff: () => 3400, seed: 46 }), STOP, 0.36, 0, { rev: 0.25 });
  put(subs, sub({ f: hz(C.root), len: end - STOP, attack: 0.8, release: 0.006 }), STOP, 0.26);
  for (let s = 0; s < 11; s++)
    put(arp, pluck({ f: hz(C.arp[ARP_SHAPE[s]]), seed: 70 + s }), STOP + s * S16, 0.13 + 0.012 * s, 0, { dly: 0.35, rev: 0.12 });
  // The landing flashes ripple out ring by ring: centre, middle, corners.
  for (const [t, n, pan] of [
    [11.75, 'G5', 0],
    [11.83, 'C6', -0.45],
    [11.91, 'D6', 0.5],
  ])
    put(fx, bell({ f: hz(n), ratio: 2, index: 1.2, decay: 0.3, idecay: 0.05, len: 1.0 }), t, 0.16, pan, { rev: 0.4 });
  // A riser, a reverse cymbal and a short roll into the breath before the mark lands.
  put(air, riser({ len: 0.95, f0: 300, f1: 9000, from: 'C3', to: 'D5', tone: 0.3, seed: 32 }), end - 0.95, 0.55);
  put(air, reverseCymbal({ len: 1.0, seed: 33 }), end - 1.0, 0.6);
  [12.0, 12.1, 12.15, 12.2, 12.25, 12.3].forEach((t, k) => put(drums, k === 5 ? ROLL_CUT : ROLL, t, 0.2 + k * 0.09, 0, { rev: 0.2 }));
}

// 12.4–15s: the lockup.
function lockup() {
  // The mark lands like a drop on water: kick, sub boom, crash and the D major chord (Picardy).
  hitKick(LOGO, KICK_BIG, 1);
  put(drums, boom({ f0: 150, f1: hz('D1'), decay: 1.0, len: 2.6, thud: 0.5, seed: 34 }), LOGO, 0.55);
  put(drums, CRASH_BIG, LOGO, 0.5, 0, { rev: 0.2 });
  const bloom = (x) => 1500 + 5200 * Math.exp(-x / 0.45);
  put(pads, pad({ notes: CHORD.D.pad, len: 1.9, attack: 0.004, release: 0.5, detune: 18, cutoff: bloom, seed: 47 }), LOGO, 0.42, 0, { rev: 0.4 });
  put(subs, sub({ f: hz(CHORD.D.root), len: 1.8, attack: 0.004, release: 0.4 }), LOGO, 0.3);
  put(fx, drop({ f0: 560, f1: hz('D6'), sweep: 0.035, decay: 0.09 }), LOGO, 0.42, 0, { rev: 0.5 });
  // The two water rings.
  put(fx, bell({ f: hz('D6'), ratio: 3.5, index: 1.8, decay: 1.0 }), 12.45, 0.26, -0.1, { rev: 0.5 });
  put(fx, bell({ f: hz('A6'), ratio: 3.5, index: 1.4, decay: 0.8 }), 12.56, 0.17, 0.1, { rev: 0.5 });
  // "Oceans LLM" rises letter by letter (0.035 stagger): a harp run up D major pentatonic.
  ['A4', 'B4', 'D5', 'E5', 'F#5', 'A5', 'B5', 'D6', 'E6'].forEach((n, k) =>
    put(fx, pluck({ f: hz(n), decay: 0.5, bright: 2600, len: 1.4, seed: 80 + k }), 12.68 + k * 0.035, 0.13, -0.15 + k * 0.08, { rev: 0.45, dly: 0.15 }),
  );
  // The tagline ripples outward from "every" (0.045 stagger from the centre): bell pairs that
  // spread in pitch and across the stereo field with each wave.
  [
    ['D6', 'E6'],
    ['A5', 'F#6'],
    ['F#5', 'A6'],
    ['D5', 'D7'],
  ].forEach(([lo, hi], k) => {
    const t = 13.33 + k * 0.045;
    const w = 0.15 + k * 0.2;
    put(fx, bell({ f: hz(lo), ratio: 3.5, index: 1.0, decay: 0.8 }), t, 0.075, -w, { rev: 0.5 });
    put(fx, bell({ f: hz(hi), ratio: 3.5, index: 1.0, decay: 0.8 }), t, 0.075, w, { rev: 0.5 });
  });
}

// ---------------------------------------------------------------------------------------------
// Mix and master.

function filter(b, fc, type = 'lp', q = 0.707) {
  const fl = new SVF();
  const fr = new SVF();
  for (let i = 0; i < b.n; i++) {
    const f = fc(i / SR);
    const l = fl.run(b.l[i], f, q)[type];
    const r = fr.run(b.r[i], f, q)[type];
    // An open low-pass passes the signal untouched; the filter keeps running so it engages cleanly.
    if (!(type === 'lp' && f >= 19000)) {
      b.l[i] = l;
      b.r[i] = r;
    }
  }
}

// Open until the exit push, closes with it (power2.in), then surfaces as the wall resolves out of
// the blur, fully open again in the breath before the logo.
function underwater(t) {
  const OPEN = 20000;
  const LOW = 420;
  const end = BREATHS[1][0];
  if (t < 10.8 || t >= end) return OPEN;
  if (t < STOP) return OPEN * (LOW / OPEN) ** (((t - 10.8) / 0.4) ** 2);
  return LOW * (9000 / LOW) ** (((t - STOP) / (end - STOP)) ** 2.2);
}

// The two breaths (6ms down to −26 dB, 3ms back up on the hit) and the fade into the last frame.
function envelope(t) {
  let g = 1;
  for (const [a, b] of BREATHS) {
    if (t >= a && t < b) g = Math.min(g, Math.max(0.05, 1 - (t - a) / 0.006, (t - (b - 0.003)) / 0.003));
  }
  if (t > 14.3) g *= Math.cos(((Math.min(t, LEN) - 14.3) / 0.7) * (Math.PI / 2)) ** 2;
  return g;
}

// The surfacing swell: the Csus bed rises from −4.4 dB into the breath before the logo.
function swell(t) {
  const end = BREATHS[1][0];
  if (t < STOP || t >= end) return 1;
  return 0.6 + 0.4 * ((t - STOP) / (end - STOP)) ** 1.5;
}

function master() {
  filter(reese, () => 110, 'hp');
  const bass = bus();
  bass.mix(reese);
  bass.mix(subs);
  // Sidechain: the bass pumps hard, the pads breathe, the arp barely moves.
  bass.scale(ducker(kicks, { depth: 0.75, tau: 0.09 }));
  keys.scale(ducker(kicks, { depth: 0.45, tau: 0.12 }));
  arp.scale(ducker(kicks, { depth: 0.25, tau: 0.1 }));
  for (const b of [bass, keys, arp]) b.scale(swell);
  const verb = freeverb(rev, { room: 0.86, damp: 0.4 });
  const echo = pingpong(dly, { time: 0.3, fb: 0.36, lp: 3800 });
  const stems = { drums, bass, keys, pads, arp, fx, air, verb, echo };
  const gains = { fx: FX_GAIN, verb: REV_RETURN, echo: DLY_RETURN };
  // The groove, with its reverb and echo, goes underwater on the exit push; accents stay dry.
  const groove = bus();
  for (const k of ['drums', 'bass', 'keys', 'pads', 'arp', 'verb', 'echo']) groove.mix(stems[k], gains[k] ?? 1);
  filter(groove, underwater, 'lp', 0.9);
  const mix = bus();
  mix.mix(groove);
  mix.mix(fx, FX_GAIN);
  mix.mix(air);
  // Nothing useful lives below 28 Hz; it would only eat headroom.
  filter(mix, () => 28, 'hp');
  mix.scale(envelope);
  return { mix, stems, gains };
}

function clone(b) {
  const c = bus();
  c.l.set(b.l);
  c.r.set(b.r);
  return c;
}

// Gain to the loudness target, limit, then correct for what the limiter shaved.
function finish(mix) {
  let gain = 10 ** ((TARGET_LUFS - lufs(mix.l, mix.r)) / 20);
  let out;
  let gr = 0;
  for (let pass = 0; pass < 4; pass++) {
    out = clone(mix);
    out.scale(() => gain);
    gr = limit(out, { ceiling: CEILING });
    const got = lufs(out.l, out.r);
    if (Math.abs(got - TARGET_LUFS) < 0.05) break;
    gain *= 10 ** ((TARGET_LUFS - got) / 20);
  }
  return { out, gain, gr };
}

const span = (b, a, z) => lufs(b.l.subarray(Math.round(a * SR), Math.round(z * SR)), b.r.subarray(Math.round(a * SR), Math.round(z * SR)));
const fmt = (x) => (Number.isFinite(x) ? x.toFixed(1).padStart(6) : '     —');

converge();
groove();
accents();
surface();
lockup();
const { mix, stems, gains } = master();

const WINDOWS = [
  ['converge', 0, DROP],
  ['flight', DROP, 10.8],
  ['lockup', LOGO, LEN],
];
console.log(`stem loudness, LU relative to the mix in each window\n${'stem'.padEnd(8)}${WINDOWS.map(([n]) => n.padStart(10)).join('')}`);
const ref = WINDOWS.map(([, a, z]) => span(mix, a, z));
for (const [k, b] of Object.entries(stems)) {
  const scaled = clone(b);
  if (gains[k]) scaled.scale(() => gains[k]);
  console.log(`${k.padEnd(8)}${WINDOWS.map(([, a, z], w) => `    ${fmt(span(scaled, a, z) - ref[w])}`).join('')}`);
}

const { out, gain, gr } = finish(mix);
mkdirSync(path.dirname(OUT), { recursive: true });
writeWav(OUT, out, LEN);
console.log(
  `\nwrote ${path.relative(ROOT, OUT)}: ${LEN}s, ${fmt(lufs(out.l, out.r))} LUFS, peak ${peakDb(out).toFixed(2)} dBFS, ` +
    `master gain ${(20 * Math.log10(gain)).toFixed(1)} dB, limiter max GR ${gr.toFixed(1)} dB, ${kicks.length} kicks`,
);
