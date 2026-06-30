import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import type { EffectiveConfig, PullRequestContext, ReviewResult } from "./types";
import { readReviewResult } from "./result-artifact";

export interface PiInvocation {
  binary: string;
  args: string[];
  resultPath: string;
  tempDir: string;
}

export function resolvePiBinary(explicit?: string): string {
  if (explicit) {
    return explicit;
  }
  const candidates = [
    resolve(__dirname, "../vendor/pi/bin/pi"),
    resolve(__dirname, "../node_modules/.bin/pi"),
    resolve(process.cwd(), "actions/review-agent/vendor/pi/bin/pi")
  ];
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) {
    throw new Error(
      "Pi runtime is not packaged with this action yet. Provide the pi-binary input or vendor a Pi runtime artifact under actions/review-agent/vendor/pi/bin/pi."
    );
  }
  return found;
}

export function preparePiInvocation(input: {
  piBinary: string;
  context: PullRequestContext;
  effectiveConfig: EffectiveConfig;
}): PiInvocation {
  const tempDir = mkdtempSync(join(tmpdir(), "oceans-review-agent-"));
  const contextPath = join(tempDir, "context.json");
  const configPath = join(tempDir, "config.json");
  const resultPath = join(tempDir, "result.json");
  writeFileSync(contextPath, JSON.stringify(input.context, null, 2));
  writeFileSync(configPath, JSON.stringify(input.effectiveConfig, null, 2));
  return {
    binary: input.piBinary,
    args: ["review", "--context", contextPath, "--config", configPath, "--output", resultPath],
    resultPath,
    tempDir
  };
}

export function invokePi(invocation: PiInvocation, timeoutMinutes: number): ReviewResult {
  execFileSync(invocation.binary, invocation.args, {
    stdio: ["ignore", "pipe", "pipe"],
    timeout: timeoutMinutes * 60_000,
    env: {
      PATH: process.env.PATH ?? "",
      HOME: process.env.HOME ?? "",
      TMPDIR: process.env.TMPDIR ?? tmpdir()
    }
  });
  return readReviewResult(invocation.resultPath);
}

export function cleanupPiInvocation(invocation: PiInvocation): void {
  rmSync(invocation.tempDir, { recursive: true, force: true });
}

export function createFakePiBinary(path: string, result: unknown): void {
  const script = `#!/usr/bin/env node
const fs = require("fs");
const outputIndex = process.argv.indexOf("--output");
if (outputIndex < 0) process.exit(2);
fs.writeFileSync(process.argv[outputIndex + 1], ${JSON.stringify(JSON.stringify(result))});
`;
  writeFileSync(path, script);
  chmodSync(path, 0o755);
  readFileSync(path, "utf8");
}
