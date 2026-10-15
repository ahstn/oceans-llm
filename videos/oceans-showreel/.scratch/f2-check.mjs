// Numeric sanity check for the Frame 2 flight: landing framing, slivers, culling and z budget.
import { PLANES, FILLERS, STOPS, planePoint, view, project } from './f2-geom.mjs';
import { camAt } from './f2-path.mjs';
import { ALL, HOLD_TIMES, screenPoly, onScreen, visibility, cullSpans, keptDust } from './f2-vis.mjs';

const r0 = (v) => Math.round(v);

// Optional layout override for quick sweeps: OVR='fc:-2000,900,-3400,12;fa:...'
for (const part of (process.env.OVR || '').split(';').filter(Boolean)) {
  const [id, nums] = part.split(':');
  const [x, y, z, a] = nums.split(',').map(Number);
  const P = [...PLANES, ...FILLERS].find((q) => q.id === id);
  P.pos = [x, y, z];
  if (Number.isFinite(a)) P.a = a;
}

console.log('== hold framing (visible area % of screen, bbox of the visible part)');
HOLD_TIMES.forEach((t, i) => {
  const pose = camAt(t);
  const parts = [];
  for (const P of ALL) {
    const { poly } = screenPoly(pose, P);
    const vis = onScreen(poly);
    if (vis.area > 0) parts.push(`${P.id} ${((vis.area / (1920 * 1080)) * 100).toFixed(1)}% [${vis.bbox}]`);
  }
  const st = STOPS[i];
  const cc = [
    [st.card.x, st.card.y],
    [st.card.x + st.card.w, st.card.y + st.card.h],
  ].map(([x, y]) => project(view(pose, planePoint(st.plane, x, y, st.lift)), pose.P));
  const sc = [
    [st.src.x, st.src.y],
    [st.src.x + st.src.w, st.src.y + st.src.h],
  ].map(([x, y]) => project(view(pose, planePoint(st.plane, x, y, 1)), pose.P));
  console.log(`stop ${i + 1} ${st.key} @${t} P ${r0(pose.P)}: ${parts.join(' | ')}`);
  console.log(`   card (${r0(cc[0][0])},${r0(cc[0][1])})-(${r0(cc[1][0])},${r0(cc[1][1])})  src (${r0(sc[0][0])},${r0(sc[0][1])})-(${r0(sc[1][0])},${r0(sc[1][1])})`);
});

console.log('\n== visibility intervals to the cut (frames @30fps where any part is within 160px of the frame)');
for (const P of ALL) {
  const { spans, kmaxShown, near, cross } = visibility(P);
  const fmt = (list) => list.map(([a, b]) => `${a.toFixed(2)}–${b.toFixed(2)}`).join(', ') || 'never';
  const nearTxt = near.length
    ? `  NEAR: ${near.slice(0, 6).map((t) => t.toFixed(2)).join(',')}${near.length > 6 ? '…' : ''}`
    : '';
  const crossTxt = cross.length ? `  CROSS: ${cross.map((t) => t.toFixed(2)).join(',')}` : '';
  console.log(`${P.id.padEnd(6)} ${fmt(spans)}  kmax-while-shown ${kmaxShown.toFixed(2)}${nearTxt}${crossTxt}`);
  console.log(`       cull ${fmt(cullSpans(P))}`);
}

console.log(`\ndust kept ${keptDust(110).length}/110`);

// Config button on the models page vs the lifted routing card just before the flip: the ring
// must land on the button, not under the card.
{
  const t = 1.85;
  const pose = camAt(t);
  const st = STOPS[1];
  const route = STOPS[0];
  const pt = (pl, x, y, w) => project(view(pose, planePoint(pl, x, y, w)), pose.P).map(r0);
  const b = [pt(0, st.src.x, st.src.y, 0), pt(0, st.src.x + st.src.w, st.src.y + st.src.h, 0)];
  const c = [pt(0, route.card.x, route.card.y, route.lift), pt(0, route.card.x + route.card.w, route.card.y + route.card.h, route.lift)];
  console.log(`\nconfig button @${t}: (${b[0].slice(0, 2)})-(${b[1].slice(0, 2)})  route card (${c[0].slice(0, 2)})-(${c[1].slice(0, 2)})`);
}

// Camera P and pose samples through the new legs.
for (const t of [1.95, 2.35, 2.75, 8.35, 8.75, 9.15, 9.8, 10.6, 10.8, 11.3, 12.8, 13.17, 13.55, 15.2, 15.6]) {
  const p = camAt(t);
  console.log(`t ${t}: P ${r0(p.P)} zs ${r0(p.zs)} rx ${p.rx.toFixed(1)} ry ${p.ry.toFixed(1)} L (${r0(p.lx)},${r0(p.ly)},${r0(p.lz)})`);
}
