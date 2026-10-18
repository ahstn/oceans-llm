// How far each transient effect pokes out of everything else, in its own band.
// For each clip id given: place its prepared WAV (from premix.mjs) at its start/volume, subtract
// it from the full mix to get "the rest", band-limit both, and compare the loudest 20 ms window of
// the effect with the rest over that same window. >= 0 dB reads clearly; -6 dB is marginal.
// Usage: node .scratch/audio/cue-snr.mjs <lo Hz> <hi Hz> <clip-id>...
import { readFileSync } from "node:fs";

const [lo, hi, ...ids] = process.argv.slice(2);
const html = readFileSync("index.html", "utf8");

function readWav(path) {
  const b = readFileSync(path);
  let p = 12, fmt, ch, data;
  while (p < b.length) {
    const id = b.toString("ascii", p, p + 4), size = b.readUInt32LE(p + 4);
    if (id === "fmt ") { fmt = b.readUInt16LE(p + 8); ch = b.readUInt16LE(p + 10); }
    if (id === "data") { data = b.subarray(p + 8, p + 8 + size); break; }
    p += 8 + size + (size & 1);
  }
  const bytes = fmt === 3 ? 4 : 2, frames = data.length / bytes / ch, out = new Float32Array(frames);
  for (let i = 0; i < frames; i++) {
    let s = 0;
    for (let c = 0; c < ch; c++) {
      const o = (i * ch + c) * bytes;
      s += fmt === 3 ? data.readFloatLE(o) : data.readInt16LE(o) / 32768;
    }
    out[i] = s / ch;
  }
  return out;
}
// RBJ biquad high-pass then low-pass, two passes each, for a steep-ish band.
function band(x, sr = 48000) {
  const run = (sig, type, f0) => {
    const w = (2 * Math.PI * f0) / sr, al = Math.sin(w) / (2 * Math.SQRT1_2), cw = Math.cos(w);
    const b0 = type === "hp" ? (1 + cw) / 2 : (1 - cw) / 2, b1 = type === "hp" ? -(1 + cw) : 1 - cw;
    const a0 = 1 + al, a1 = -2 * cw, a2 = 1 - al, y = new Float32Array(sig.length);
    let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
    for (let i = 0; i < sig.length; i++) {
      const v = (b0 * sig[i] + b1 * x1 + b0 * x2 - a1 * y1 - a2 * y2) / a0;
      x2 = x1; x1 = sig[i]; y2 = y1; y1 = v; y[i] = v;
    }
    return y;
  };
  return run(run(run(run(x, "hp", +lo), "hp", +lo), "lp", +hi), "lp", +hi);
}
const rms = (x, a, n) => { let s = 0; for (let i = a; i < a + n; i++) s += x[i] * x[i]; return Math.sqrt(s / n); };
const db = (v) => (20 * Math.log10(v + 1e-12)).toFixed(1).padStart(6);

const mix = readWav(".scratch/audio/premix/mix.wav");
const win = 960; // 20 ms
for (const id of ids) {
  const tag = html.match(new RegExp(`<audio[^>]*id="${id}"[^>]*>`))[0];
  const start = Math.round(Number(tag.match(/data-start="([\d.]+)"/)[1]) * 48000);
  const vol = Number(tag.match(/data-volume="([\d.]+)"/)[1]);
  const clip = readWav(`.scratch/audio/premix/${id}.wav`);
  const placed = new Float32Array(mix.length), rest = new Float32Array(mix.length);
  for (let i = 0; i < clip.length && start + i < mix.length; i++) placed[start + i] = clip[i] * vol;
  for (let i = 0; i < mix.length; i++) rest[i] = mix[i] - placed[i];
  const fx = band(placed), bg = band(rest);
  let best = start, bestV = 0;
  for (let a = start; a + win < Math.min(mix.length, start + clip.length); a += 120) {
    const v = rms(fx, a, win);
    if (v > bestV) { bestV = v; best = a; }
  }
  const restV = rms(bg, best, win);
  console.log(`${id.padEnd(16)} peak@${(best / 48000).toFixed(3)}s  fx ${db(bestV)} dB  rest ${db(restV)} dB  margin ${db(bestV / restV)} dB`);
}
