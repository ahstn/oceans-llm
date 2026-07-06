import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const distPath = resolve("dist/index.js");
let before;
try {
  before = readFileSync(distPath, "utf8");
} catch {
  console.error("dist/index.js is missing; run bun run build.");
  process.exit(1);
}

execFileSync("bun", ["run", "build"], { stdio: "inherit" });
const after = readFileSync(distPath, "utf8");
if (before !== after) {
  console.error("dist/index.js is stale; run mise run review-agent-action-build.");
  process.exit(1);
}
