#!/usr/bin/env node

import { analyzeRepository } from "./release-common.mjs";

function parseArgs(argv) {
  let json = false;

  for (const arg of argv) {
    if (arg === "--json") {
      json = true;
      continue;
    }

    throw new Error(`unknown argument: ${arg}`);
  }

  return { json };
}

function formatNoReleaseMessage(result) {
  if (result.previousTag) {
    return `No releasable changes found since ${result.previousTag}.`;
  }

  return "No releasable changes found in repository history.";
}

const options = parseArgs(process.argv.slice(2));
const result = analyzeRepository();

if (options.json) {
  console.log(JSON.stringify(result, null, 2));
  process.exit(0);
}

if (!result.hasRelease) {
  console.log(formatNoReleaseMessage(result));
  process.exit(0);
}

console.log(`Next release: ${result.tag}`);
console.log(`Release type: ${result.releaseType}`);
console.log("");

for (const commit of result.commits) {
  console.log(`- ${commit.subject}`);
}
