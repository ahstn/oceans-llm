import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const distPath = resolve("dist/index.js");
const source = readFileSync(distPath, "utf8");
const normalized = source
  .split("\n")
  .map((line) => line.trimEnd())
  .join("\n");

writeFileSync(distPath, normalized.endsWith("\n") ? normalized : `${normalized}\n`);
