// Small offline synth for the showreel score: band-limited oscillators, a zero-delay-feedback
// state-variable filter, drum and tonal voices, Freeverb, a ping-pong delay, a BS.1770 loudness
// meter and a 16-bit WAV writer. Every noise source is seeded, so the score renders identically
// on every run.
import { writeFileSync } from 'node:fs';

export const SR = 48000;
const TAU = Math.PI * 2;

// mulberry32: tiny seeded PRNG in [0, 1).
export function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PITCH = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
export function hz(name) {
  const m = /^([A-G])([#b]?)(-?\d)$/.exec(name);
  if (!m) throw new Error(`bad note ${name}`);
  const n = 12 * (Number(m[3]) + 1) + PITCH[m[1]] + (m[2] === '#' ? 1 : m[2] === 'b' ? -1 : 0);
  return 440 * 2 ** ((n - 69) / 12);
}
export const cents = (c) => 2 ** (c / 1200);

// ---------------------------------------------------------------------------------------------
// Oscillators and filters.

// PolyBLEP residual for a unit step at phase 0; removes most aliasing from saw and pulse edges.
function blep(t, dt) {
  if (t < dt) {
    const x = t / dt;
    return x + x - x * x - 1;
  }
  if (t > 1 - dt) {
    const x = (t - 1) / dt;
    return x * x + x + x + 1;
  }
  return 0;
}

export class Saw {
  constructor(phase = 0) {
    this.p = phase;
  }
  next(f) {
    const dt = f / SR;
    const v = 2 * this.p - 1 - blep(this.p, dt);
    this.p += dt;
    if (this.p >= 1) this.p -= 1;
    return v;
  }
}

export class Pulse {
  constructor(phase = 0, width = 0.5) {
    this.p = phase;
    this.w = width;
  }
  next(f) {
    const dt = f / SR;
    let q = this.p - this.w;
    if (q < 0) q += 1;
    const v = (this.p < this.w ? 1 : -1) + blep(this.p, dt) - blep(q, dt);
    this.p += dt;
    if (this.p >= 1) this.p -= 1;
    return v;
  }
}

// Simper's trapezoidal SVF: stable under per-sample cutoff modulation. run() fills lp/bp/hp.
export class SVF {
  constructor() {
    this.ic1 = 0;
    this.ic2 = 0;
    this.lp = 0;
    this.bp = 0;
    this.hp = 0;
  }
  run(x, fc, q = 0.707) {
    const g = Math.tan((Math.PI * Math.min(Math.max(fc, 10), SR * 0.45)) / SR);
    const k = 1 / q;
    const a1 = 1 / (1 + g * (g + k));
    const a2 = g * a1;
    const a3 = g * a2;
    const v3 = x - this.ic2;
    const v1 = a1 * this.ic1 + a2 * v3;
    const v2 = this.ic2 + a2 * this.ic1 + a3 * v3;
    this.ic1 = 2 * v1 - this.ic1;
    this.ic2 = 2 * v2 - this.ic2;
    this.lp = v2;
    this.bp = v1;
    this.hp = x - k * v1 - v2;
    return this;
  }
}

// Linear attack, then exponential decay with time constant tau.
export const ad = (t, a, tau) => (t < 0 ? 0 : t < a ? t / a : Math.exp(-(t - a) / tau));
// Linear attack, hold to `len`, exponential release.
export const asr = (t, a, len, rel) => (t < 0 ? 0 : t < a ? t / a : t < len ? 1 : Math.exp(-(t - len) / rel));
// 5 ms linear fade over the last samples of a buffer, so no voice ends on a click.
const tail = (i, n) => Math.min(1, (n - i) / (0.005 * SR));
const buf = (seconds) => new Float32Array(Math.max(1, Math.round(seconds * SR)));

// ---------------------------------------------------------------------------------------------
// Drums.

export function kick({ f0 = 150, f1 = 48, pd = 0.032, decay = 0.24, len = 0.5, click = 0.45, drive = 1.8, seed = 1 } = {}) {
  const out = buf(len);
  const rand = rng(seed);
  const hp = new SVF();
  let ph = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    ph += (f1 + (f0 - f1) * Math.exp(-t / pd)) / SR;
    const body = Math.sin(TAU * ph) * ad(t, 0.0012, decay);
    const clk = hp.run(rand() * 2 - 1, 2500, 0.7).hp * Math.exp(-t / 0.004) * click;
    out[i] = (Math.tanh(drive * (body + clk)) / Math.tanh(drive)) * tail(i, out.length);
  }
  return out;
}

export function snare({ tone = 185, decay = 0.15, len = 0.45, body = 0.5, noise = 0.95, seed = 2 } = {}) {
  const out = buf(len);
  const rand = rng(seed);
  const hp = new SVF();
  const bp = new SVF();
  let p1 = 0;
  let p2 = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    const bend = 1 + 0.55 * Math.exp(-t / 0.01);
    p1 += (tone * bend) / SR;
    p2 += (tone * 1.72 * bend) / SR;
    const b = (Math.sin(TAU * p1) + 0.45 * Math.sin(TAU * p2)) * ad(t, 0.001, 0.05);
    const w = hp.run(rand() * 2 - 1, 1100, 0.6).hp;
    const nz = bp.run(w, 3600, 0.6).bp * 1.4 + w * 0.45;
    let clap = 0;
    for (const o of [0, 0.009, 0.019]) if (t >= o) clap = Math.max(clap, 0.75 * Math.exp(-(t - o) / 0.0035));
    const env = Math.max(ad(t, 0.0008, decay), clap);
    out[i] = Math.tanh(1.4 * (b * body + nz * env * noise)) * tail(i, out.length);
  }
  return out;
}

export function hat({ decay = 0.035, len = 0.3, tone = 10500, seed = 3 } = {}) {
  const out = buf(len);
  const rand = rng(seed);
  const hp = new SVF();
  const bp = new SVF();
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    const w = hp.run(rand() * 2 - 1, 7200, 0.8).hp;
    const v = w * 0.7 + bp.run(w, tone, 1.3).bp * 0.8;
    out[i] = v * ad(t, 0.0006, decay) * tail(i, out.length);
  }
  return out;
}

// Stereo cymbal: independent noise per side, so it spreads wide.
export function crash({ len = 2.6, decay = 0.85, lp = 11000, seed = 4 } = {}) {
  const side = (s) => {
    const out = buf(len);
    const rand = rng(s);
    const hp = new SVF();
    const bp = new SVF();
    const lo = new SVF();
    for (let i = 0; i < out.length; i++) {
      const t = i / SR;
      const w = hp.run(rand() * 2 - 1, 3400, 0.7).hp;
      const v = lo.run(w * 0.6 + bp.run(w, 7200, 0.45).bp * 0.9, lp, 0.6).lp;
      const env = ad(t, 0.002, decay) * 0.75 + 0.5 * Math.exp(-t / 0.05);
      out[i] = v * env * tail(i, out.length);
    }
    return out;
  };
  return { l: side(seed), r: side(seed + 101) };
}

// A cymbal swell that ends exactly at its last sample (place it so the end hits the target).
export function reverseCymbal({ len = 1.0, seed = 5 } = {}) {
  const c = crash({ len: len + 0.2, decay: 1.4, lp: 12000, seed });
  const n = Math.round(len * SR);
  const flip = (src) => {
    const out = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const p = (i + 1) / n;
      out[i] = src[n - 1 - i] * p ** 2.2 * tail(i, n);
    }
    return out;
  };
  return { l: flip(c.l), r: flip(c.r) };
}

// ---------------------------------------------------------------------------------------------
// Low end.

// Pitched impact: a sine that falls onto f1, a 2nd harmonic for small speakers, and a noise thud.
export function boom({ f0 = 110, f1 = 36.7, pd = 0.07, decay = 0.55, len = 2.0, thud = 0.45, seed = 6 } = {}) {
  const out = buf(len);
  const rand = rng(seed);
  const lp = new SVF();
  let ph = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    ph += (f1 + (f0 - f1) * Math.exp(-t / pd)) / SR;
    const s = Math.sin(TAU * ph) + 0.28 * Math.sin(2 * TAU * ph) * Math.exp(-t / (decay * 0.6));
    const th = lp.run(rand() * 2 - 1, 700, 0.8).lp * Math.exp(-t / 0.06) * thud;
    out[i] = (Math.tanh(1.3 * (s * ad(t, 0.002, decay) + th)) / Math.tanh(1.3)) * tail(i, out.length);
  }
  return out;
}

// Sustained sub note with a touch of 2nd harmonic so it survives laptop speakers.
export function sub({ f, len, attack = 0.01, release = 0.06 } = {}) {
  const out = buf(len + release * 5);
  let ph = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    ph += f / SR;
    out[i] = (Math.sin(TAU * ph) + 0.18 * Math.sin(2 * TAU * ph)) * asr(t, attack, len, release) * tail(i, out.length);
  }
  return out;
}

// Rolling 16th bass: two detuned saws through a plucked low-pass.
export function bassPluck({ f, len = 0.085, seed = 7 } = {}) {
  const out = buf(len + 0.08);
  const rand = rng(seed);
  const a = new Saw(rand());
  const b = new Saw(rand());
  const lp = new SVF();
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    const x = a.next(f * cents(-7)) + b.next(f * cents(7));
    const v = lp.run(x, 170 + 1500 * Math.exp(-t / 0.045), 1.1).lp;
    out[i] = Math.tanh(1.6 * v * asr(t, 0.002, len, 0.015)) * tail(i, out.length);
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// Tonal voices.

// Supersaw chord: `voices` detuned saws per note spread across the stereo field, low-passed by
// cutoff(t). Returns {l, r}, `len` seconds plus the release tail.
export function pad({ notes, len, attack = 0.3, release = 0.5, voices = 7, detune = 22, width = 0.85, cutoff = () => 2200, q = 0.7, seed = 8 }) {
  const n = Math.round((len + release * 5) * SR);
  const l = new Float32Array(n);
  const r = new Float32Array(n);
  const rand = rng(seed);
  const norm = 1 / Math.sqrt(voices * notes.length);
  notes.forEach((name, k) => {
    const f = hz(name);
    for (let v = 0; v < voices; v++) {
      const x = voices === 1 ? 0 : (v / (voices - 1)) * 2 - 1;
      const ratio = cents(detune * x * (0.85 + 0.3 * rand()));
      const pan = width * x * (k % 2 ? -1 : 1);
      const gl = Math.cos(((pan + 1) * Math.PI) / 4) * Math.SQRT2 * norm;
      const gr = Math.sin(((pan + 1) * Math.PI) / 4) * Math.SQRT2 * norm;
      const osc = new Saw(rand());
      for (let i = 0; i < n; i++) {
        const s = osc.next(f * ratio);
        l[i] += s * gl;
        r[i] += s * gr;
      }
    }
  });
  const fl = new SVF();
  const fr = new SVF();
  for (let i = 0; i < n; i++) {
    const t = i / SR;
    const fc = cutoff(t);
    const e = asr(t, attack, len, release) * tail(i, n);
    l[i] = fl.run(l[i], fc, q).lp * e;
    r[i] = fr.run(r[i], fc, q).lp * e;
  }
  return { l, r };
}

// Plucked arp voice: saw + narrow pulse through a snapping low-pass.
export function pluck({ f, decay = 0.17, bright = 4800, len = 0.7, seed = 9 } = {}) {
  const out = buf(len);
  const rand = rng(seed);
  const a = new Saw(rand());
  const b = new Pulse(rand(), 0.32);
  const lp = new SVF();
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    const x = a.next(f) + 0.55 * b.next(f * cents(5));
    out[i] = lp.run(x, 380 + bright * Math.exp(-t / 0.06), 1.4).lp * ad(t, 0.001, decay) * 0.6 * tail(i, out.length);
  }
  return out;
}

// Two-operator FM bell; ratio 3.5 rings like a bell, 1–2 sounds like glass or a soft sine tone.
export function bell({ f, ratio = 3.5, index = 2.6, decay = 1.1, idecay = 0.4, glass = 0.25, len = 2.4 } = {}) {
  const out = buf(len);
  let pc = 0;
  let pm = 0;
  let pg = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    pm += (f * ratio) / SR;
    pc += f / SR;
    pg += (f * 2.01) / SR;
    const mod = index * Math.exp(-t / idecay) * Math.sin(TAU * pm);
    const v = Math.sin(TAU * pc + mod) + glass * Math.sin(TAU * pg) * Math.exp(-t / (decay * 0.3));
    out[i] = v * ad(t, 0.0015, decay) * 0.7 * tail(i, out.length);
  }
  return out;
}

// Water drop: a sine that glides up fast and dies away.
export function drop({ f0 = 640, f1 = 1175, sweep = 0.03, decay = 0.075, len = 0.45 } = {}) {
  const out = buf(len);
  let ph = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    ph += (f0 * (f1 / f0) ** Math.min(1, t / sweep)) / SR;
    out[i] = Math.sin(TAU * ph) * ad(t, 0.0008, decay) * tail(i, out.length);
  }
  return out;
}

// Riser: stereo band-passed noise and a saw glide, both climbing to the last sample.
export function riser({ len, f0 = 350, f1 = 7000, from = 'C3', to = 'C5', tone = 0.35, seed = 10 } = {}) {
  const n = Math.round(len * SR);
  const l = new Float32Array(n);
  const r = new Float32Array(n);
  const rl = rng(seed);
  const rr = rng(seed + 7);
  const [bl, br, tl] = [new SVF(), new SVF(), new SVF()];
  const saw = new Saw(0);
  const [p0, p1] = [hz(from), hz(to)];
  for (let i = 0; i < n; i++) {
    const p = (i + 1) / n;
    const fc = f0 * (f1 / f0) ** p;
    const s = tl.run(saw.next(p0 * (p1 / p0) ** (p * p)), fc * 0.9, 0.9).lp * tone;
    const e = p ** 2.2 * tail(i, n);
    l[i] = (bl.run(rl() * 2 - 1, fc, 1.4).bp + s) * e;
    r[i] = (br.run(rr() * 2 - 1, fc * 1.03, 1.4).bp + s) * e;
  }
  return { l, r };
}

// UI blip: a sine click with a fast decay (counter ticks).
export function blip({ f, decay = 0.01, len = 0.06 } = {}) {
  const out = buf(len);
  let ph = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    ph += f / SR;
    out[i] = Math.sin(TAU * ph) * ad(t, 0.0005, decay) * tail(i, out.length);
  }
  return out;
}

// Alert beep: a rounded square tone with a short hold.
export function beep({ f, len = 0.06 } = {}) {
  const out = buf(len + 0.05);
  const osc = new Pulse(0, 0.5);
  const lp = new SVF();
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    out[i] = lp.run(osc.next(f), 3500, 0.7).lp * asr(t, 0.003, len, 0.008) * 0.6 * tail(i, out.length);
  }
  return out;
}

// "Denied": a minor-second pulse cluster that wobbles with the chip's rejection shake.
export function buzz({ f, len = 0.5, wobble = 18 } = {}) {
  const out = buf(len);
  const a = new Pulse(0, 0.5);
  const b = new Pulse(0.3, 0.5);
  const lp = new SVF();
  let ph = 0;
  for (let i = 0; i < out.length; i++) {
    const t = i / SR;
    ph += (60 + 70 * Math.exp(-t / 0.02)) / SR;
    const thunk = Math.sin(TAU * ph) * Math.exp(-t / 0.07);
    const x = lp.run(a.next(f) + b.next(f * 2 ** (1 / 12)), 1500, 0.9).lp * 0.5;
    const trem = 1 - 0.45 * (0.5 + 0.5 * Math.cos(TAU * wobble * t));
    out[i] = (x * trem * ad(t, 0.003, 0.17) + thunk * 0.8) * tail(i, out.length);
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// Buses, effects and mastering.

export class Bus {
  constructor(seconds) {
    this.n = Math.ceil(seconds * SR);
    this.l = new Float32Array(this.n);
    this.r = new Float32Array(this.n);
  }
  // Mono source at time t, equal-power pan normalised to unity in the centre.
  mono(src, t, gain = 1, pan = 0) {
    const a = ((pan + 1) * Math.PI) / 4;
    this.stereo(src, src, t, gain * Math.cos(a) * Math.SQRT2, gain * Math.sin(a) * Math.SQRT2);
  }
  stereo(l, r, t, gl = 1, gr = gl) {
    const i0 = Math.round(t * SR);
    for (let i = 0; i < l.length; i++) {
      const j = i0 + i;
      if (j < 0 || j >= this.n) continue;
      this.l[j] += l[i] * gl;
      this.r[j] += r[i] * gr;
    }
  }
  mix(other, gain = 1) {
    for (let i = 0; i < this.n; i++) {
      this.l[i] += other.l[i] * gain;
      this.r[i] += other.r[i] * gain;
    }
  }
  scale(fn) {
    for (let i = 0; i < this.n; i++) {
      const g = fn(i / SR);
      this.l[i] *= g;
      this.r[i] *= g;
    }
  }
}

// Freeverb (Jezar's tunings scaled to 48 kHz) with pre-delay and a band-limited input.
export function freeverb(send, { room = 0.86, damp = 0.35, width = 1, predelay = 0.022, hp = 220, lp = 7500 } = {}) {
  const scale = SR / 44100;
  const combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
  const aps = [556, 441, 341, 225];
  const mk = (len) => ({ b: new Float32Array(Math.round(len * scale)), i: 0, s: 0 });
  const side = (spread) => ({ c: combs.map((d) => mk(d + spread)), a: aps.map((d) => mk(d + spread)) });
  const L = side(0);
  const R = side(23);
  const fb = room * 0.28 + 0.7;
  const dm = damp * 0.4;
  const pre = new Float32Array(Math.round(predelay * SR) + 1);
  let pi = 0;
  const [hpf, lpf] = [new SVF(), new SVF()];
  const out = new Bus(send.n / SR);
  const runSide = (S, x) => {
    let acc = 0;
    for (const c of S.c) {
      const y = c.b[c.i];
      c.s = y * (1 - dm) + c.s * dm;
      c.b[c.i] = x + c.s * fb;
      c.i = (c.i + 1) % c.b.length;
      acc += y;
    }
    for (const a of S.a) {
      const b = a.b[a.i];
      a.b[a.i] = acc + b * 0.5;
      a.i = (a.i + 1) % a.b.length;
      acc = b - acc;
    }
    return acc;
  };
  const w1 = width / 2 + 0.5;
  const w2 = (1 - width) / 2;
  for (let i = 0; i < send.n; i++) {
    const x = (send.l[i] + send.r[i]) * 0.5;
    const d = pre[pi];
    pre[pi] = lpf.run(hpf.run(x, hp, 0.7).hp, lp, 0.7).lp;
    pi = (pi + 1) % pre.length;
    const yl = runSide(L, d * 0.03);
    const yr = runSide(R, d * 0.03);
    out.l[i] = yl * w1 + yr * w2;
    out.r[i] = yr * w1 + yl * w2;
  }
  return out;
}

// Ping-pong delay: left tap feeds right and back, with a darkening low-pass in the loop.
export function pingpong(send, { time = 0.3, fb = 0.38, lp = 4200 } = {}) {
  const n = Math.round(time * SR);
  const dl = new Float32Array(n);
  const dr = new Float32Array(n);
  const [fl, fr] = [new SVF(), new SVF()];
  const out = new Bus(send.n / SR);
  let k = 0;
  for (let i = 0; i < send.n; i++) {
    const x = (send.l[i] + send.r[i]) * 0.5;
    const yl = dl[k];
    const yr = dr[k];
    dl[k] = x + fl.run(yr, lp, 0.6).lp * fb;
    dr[k] = fr.run(yl, lp, 0.6).lp * fb;
    k = (k + 1) % n;
    out.l[i] = yl;
    out.r[i] = yr;
  }
  return out;
}

// Sidechain gain curve: ducks by `depth` at each hit and recovers with time constant tau.
export function ducker(hits, { depth = 0.6, tau = 0.11, attack = 0.004 } = {}) {
  return (t) => {
    let g = 1;
    for (const h of hits) {
      const dt = t - h;
      if (dt < 0 || dt > tau * 7) continue;
      g = Math.min(g, 1 - depth * Math.min(1, dt / attack) * Math.exp(-dt / tau));
    }
    return g;
  };
}

// ITU-R BS.1770-4 integrated loudness (48 kHz K-weighting, 400 ms blocks, both gates).
export function lufs(l, r) {
  const kw = (x) => {
    const y = new Float64Array(x.length);
    const stages = [
      [[1.53512485958697, -2.69169618940638, 1.19839281085285], [-1.69065929318241, 0.73248077421585]],
      [[1, -2, 1], [-1.99004745483398, 0.99007225036621]],
    ];
    let src = x;
    for (const [[b0, b1, b2], [a1, a2]] of stages) {
      let [x1, x2, y1, y2] = [0, 0, 0, 0];
      for (let i = 0; i < src.length; i++) {
        const v = b0 * src[i] + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        [x2, x1, y2, y1] = [x1, src[i], y1, v];
        y[i] = v;
      }
      src = y.slice();
    }
    return src;
  };
  const [zl, zr] = [kw(l), kw(r)];
  const blk = Math.round(0.4 * SR);
  const hop = Math.round(0.1 * SR);
  const pows = [];
  for (let s = 0; s + blk <= zl.length; s += hop) {
    let p = 0;
    for (let i = s; i < s + blk; i++) p += zl[i] * zl[i] + zr[i] * zr[i];
    pows.push(p / blk);
  }
  const loud = (p) => -0.691 + 10 * Math.log10(p);
  const abs = pows.filter((p) => loud(p) > -70);
  const rel = loud(abs.reduce((a, b) => a + b, 0) / abs.length) - 10;
  const gated = abs.filter((p) => loud(p) > rel);
  return loud(gated.reduce((a, b) => a + b, 0) / gated.length);
}

// Look-ahead peak limiter: gain falls over the look-ahead window and recovers on `release`.
export function limit(bus, { ceiling = 0.84, lookahead = 0.005, release = 0.09 } = {}) {
  const la = Math.round(lookahead * SR);
  const need = new Float32Array(bus.n);
  for (let i = 0; i < bus.n; i++) {
    const p = Math.max(Math.abs(bus.l[i]), Math.abs(bus.r[i]));
    need[i] = p > ceiling ? ceiling / p : 1;
  }
  const rel = Math.exp(-1 / (release * SR));
  const att = Math.exp(-1 / (la / 3));
  let g = 1;
  let minGain = 1;
  for (let i = 0; i < bus.n; i++) {
    let target = 1;
    for (let j = i; j < Math.min(bus.n, i + la); j++) target = Math.min(target, need[j]);
    g = target < g ? target + (g - target) * att : target + (g - target) * rel;
    const gi = Math.min(g, need[i]);
    minGain = Math.min(minGain, gi);
    bus.l[i] *= gi;
    bus.r[i] *= gi;
  }
  return 20 * Math.log10(minGain);
}

export function peakDb(bus) {
  let p = 0;
  for (let i = 0; i < bus.n; i++) p = Math.max(p, Math.abs(bus.l[i]), Math.abs(bus.r[i]));
  return 20 * Math.log10(p);
}

// 16-bit PCM WAV with seeded TPDF dither.
export function writeWav(path, bus, seconds) {
  const n = Math.min(bus.n, Math.round(seconds * SR));
  const data = Buffer.alloc(n * 4);
  const rand = rng(99);
  for (let i = 0; i < n; i++) {
    for (const [c, ch] of [
      [0, bus.l],
      [1, bus.r],
    ]) {
      const d = (rand() - rand()) / 32768;
      const v = Math.max(-1, Math.min(1, ch[i] + d));
      data.writeInt16LE(Math.round(v * 32767), i * 4 + c * 2);
    }
  }
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write('WAVE', 8);
  h.write('fmt ', 12);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(2, 22);
  h.writeUInt32LE(SR, 24);
  h.writeUInt32LE(SR * 4, 28);
  h.writeUInt16LE(4, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(data.length, 40);
  writeFileSync(path, Buffer.concat([h, data]));
}
