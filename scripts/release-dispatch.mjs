#!/usr/bin/env node

import { analyzeRepository, getCurrentBranch, runCommand } from "./release-common.mjs";

const branch = getCurrentBranch();
if (branch !== "main") {
  throw new Error(`release workflow must be dispatched from main, got ${branch}`);
}

const result = analyzeRepository();
if (!result.hasRelease) {
  const baseline = result.previousTag ?? "repository start";
  console.log(`No releasable changes found since ${baseline}; skipping workflow dispatch.`);
  process.exit(0);
}

runCommand("gh", ["workflow", "run", "release.yml", "--ref", "main"], {
  stdio: "inherit",
});

console.log(`Dispatched release workflow for ${result.tag}.`);
