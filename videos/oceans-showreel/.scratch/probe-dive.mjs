import { PLANES, view, project } from './f2-geom.mjs';
import { camAt } from './f2-path.mjs';
import { quad, screenPoly, onScreen } from './f2-vis.mjs';
const G = PLANES[2];
for (let f = 320; f <= 330; f++) {
  const t = f / 30;
  const pose = camAt(t);
  const q = quad(G).map((c) => view(pose, c));
  const sp = screenPoly(pose, G);
  const on = onScreen(sp.poly);
  console.log(t.toFixed(3), 'P', pose.P.toFixed(0), 'corner qz', q.map((p) => p[2].toFixed(0)).join(','), 'kmax', sp.kmax.toFixed(2), 'on-screen', (on.area / (1920 * 1080) * 100).toFixed(1) + '%', on.bbox);
}
