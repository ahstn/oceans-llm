import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const tmp = mkdtempSync(join(tmpdir(), "oceans-action-smoke-"));
const actionDir = join(tmp, "actions/review-agent");
mkdirSync(actionDir, { recursive: true });
cpSync(resolve("dist"), join(actionDir, "dist"), { recursive: true });
cpSync(resolve("package.json"), join(actionDir, "package.json"));

const eventPath = join(tmp, "event.json");
const summaryPath = join(tmp, "summary.md");
writeFileSync(eventPath, "{}");
writeFileSync(summaryPath, "");

execFileSync("node", [join(actionDir, "dist/index.js")], {
  cwd: tmp,
  stdio: "pipe",
  env: {
    ...process.env,
    "INPUT_OCEANS-URL": "http://127.0.0.1:1",
    "INPUT_OCEANS-API-KEY": "smoke-key",
    "INPUT_DRY-RUN": "true",
    GITHUB_EVENT_NAME: "push",
    GITHUB_EVENT_PATH: eventPath,
    GITHUB_REPOSITORY: "owner/repo",
    GITHUB_TOKEN: "smoke-github-token",
    GITHUB_WORKSPACE: tmp,
    GITHUB_STEP_SUMMARY: summaryPath
  }
});
