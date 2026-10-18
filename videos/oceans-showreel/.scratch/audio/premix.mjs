// Rebuilds the renderer's audio mix (hyperframes 0.8.144) from index.html, so volumes can be
// tuned without a full video render. Mirrors prepareAudioTrack -> mixGroupMembers -> mixAudioTracks:
// per-clip trim/volume/fades/delay, a unity-gain float sum per <hf-audio-group>, then a unity-gain
// sum of everything, encoded AAC 192k. Writes .scratch/audio/premix.m4a.
// Usage (from the project dir): node .scratch/audio/premix.mjs
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const OUT = ".scratch/audio/premix";
mkdirSync(OUT, { recursive: true });
const html = readFileSync("index.html", "utf8");
const total = Number(html.match(/id="root"[^>]*data-duration="([\d.]+)"/)[1]);
const attr = (tag, name, fallback) => {
  const m = tag.match(new RegExp(`\\s${name}="([^"]*)"`));
  return m ? m[1] : fallback;
};
const groups = new Map(
  [...html.matchAll(/<hf-audio-group\b[^>]*>/g)].map(([tag]) => [attr(tag, "id"), Number(attr(tag, "data-volume", "1"))]),
);
const clips = [...html.matchAll(/<audio\b[^>]*>/g)].map(([tag]) => ({
  id: attr(tag, "id"),
  src: attr(tag, "src"),
  start: Number(attr(tag, "data-start")),
  duration: Number(attr(tag, "data-duration")),
  mediaStart: Number(attr(tag, "data-media-start", "0")),
  volume: Number(attr(tag, "data-volume", "1")),
  fadeIn: Number(attr(tag, "data-fade-in", "0")),
  fadeOut: Number(attr(tag, "data-fade-out", "0")),
  group: attr(tag, "data-audio-group"),
}));

const ff = (args) => execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args]);
const n = (x) => String(Math.round(x * 1e6) / 1e6);
const trackFilter = (c, i) => {
  const fades = [];
  if (c.fadeIn > 0) fades.push(`afade=t=in:st=0:d=${n(Math.min(c.fadeIn, c.duration))}`);
  if (c.fadeOut > 0) fades.push(`afade=t=out:st=${n(Math.max(0, c.duration - c.fadeOut))}:d=${n(Math.min(c.fadeOut, c.duration))}`);
  const ms = Math.round(c.start * 1e3);
  return `[${i}:a]atrim=0:${n(c.duration)},volume=${n(c.volume)}${fades.map((f) => "," + f).join("")},adelay=${ms}|${ms},apad,asetpts=N/SR/TB,atrim=0:${n(total)}[a${i}]`;
};
const sum = (tracks, out, codec) => {
  const graph = [
    ...tracks.map(trackFilter),
    `${tracks.map((_, i) => `[a${i}]`).join("")}amix=inputs=${tracks.length}:duration=longest:dropout_transition=0[mixed];[mixed]volume=${tracks.length}[out]`,
  ].join(";");
  writeFileSync(`${out}.graph.txt`, graph);
  ff([...tracks.flatMap((t) => ["-i", t.path]), "-/filter_complex", `${out}.graph.txt`, "-map", "[out]", ...codec, "-t", n(total), out]);
};

for (const c of clips) {
  c.path = join(OUT, `${c.id}.wav`);
  ff(["-ss", n(c.mediaStart), "-t", n(c.duration), "-i", c.src, "-acodec", "pcm_s16le", "-ar", "48000", "-ac", "2", c.path]);
}
const master = clips.filter((c) => !c.group);
for (const [id, volume] of groups) {
  const members = clips.filter((c) => c.group === id);
  const path = join(OUT, `group-${id}.wav`);
  sum(members, path, ["-acodec", "pcm_f32le", "-ar", "48000"]);
  master.push({ id, path, start: 0, duration: total, volume, fadeIn: 0, fadeOut: 0 });
}
sum(master, join(OUT, "mix.wav"), ["-acodec", "pcm_f32le", "-ar", "48000"]);
sum(master, ".scratch/audio/premix.m4a", ["-acodec", "aac", "-b:a", "192k"]);
console.log(`mixed ${clips.length} clips (${groups.size} group) -> .scratch/audio/premix.m4a and ${OUT}/mix.wav`);
