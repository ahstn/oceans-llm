// Envelope report for the bundled SFX: onset, peak, tail and a coarse sparkline, so each
// cue can be placed by its transient rather than by its file start.
import { execFileSync } from 'node:child_process';
const DIR = '/Users/ahstn/.claude/skills/media-use/audio/assets/sfx';
const names = process.argv.slice(2);
const SR = 48000;
const bars = ' ▁▂▃▄▅▆▇█';
for (const n of names) {
  const buf = execFileSync('ffmpeg', ['-v', 'error', '-i', `${DIR}/${n}.mp3`, '-ac', '1', '-ar', String(SR), '-f', 'f32le', '-'], { maxBuffer: 1 << 28 });
  const x = new Float32Array(buf.buffer, buf.byteOffset, buf.length / 4);
  const win = SR / 100; // 10 ms
  const env = [];
  let zc = 0;
  for (let i = 0; i + win <= x.length; i += win) {
    let s = 0;
    for (let j = i; j < i + win; j++) s += x[j] * x[j];
    env.push(10 * Math.log10(s / win + 1e-12));
  }
  for (let i = 1; i < x.length; i++) if ((x[i - 1] < 0) !== (x[i] < 0)) zc++;
  let pk = -Infinity, pi = 0, absPk = 0;
  env.forEach((v, i) => { if (v > pk) { pk = v; pi = i; } });
  for (const v of x) absPk = Math.max(absPk, Math.abs(v));
  const on = env.findIndex((v) => v > pk - 20);
  let tail = env.length - 1;
  while (tail > 0 && env[tail] < pk - 30) tail--;
  const step = Math.max(1, Math.round(env.length / 60));
  let spark = '';
  for (let i = 0; i < env.length; i += step) {
    const v = Math.max(...env.slice(i, i + step));
    spark += bars[Math.max(0, Math.min(8, Math.round(((v - (pk - 48)) / 48) * 8)))];
  }
  console.log(`${n.padEnd(17)} len ${(x.length / SR).toFixed(2)}s onset ${(on / 100).toFixed(2)} peak ${(pi / 100).toFixed(2)} (${pk.toFixed(1)} dB rms, ${(20 * Math.log10(absPk)).toFixed(1)} dBFS) tail ${(tail / 100).toFixed(2)} zcr ${Math.round(zc / (x.length / SR))}/s  step ${step * 10}ms`);
  console.log(`  ${spark}`);
}
