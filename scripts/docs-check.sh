#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

node <<'NODE'
const fs = require("fs");
const path = require("path");

const repoRoot = process.cwd();
const crossCuttingPages = new Set([
  "docs/request-lifecycle-and-failure-modes.md",
  "docs/runtime-bootstrap-and-access.md",
  "docs/operator-runbooks.md",
  "docs/oidc-and-sso-status.md",
  "docs/admin-api-contract-workflow.md",
]);

function walk(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walk(full));
    } else if (entry.isFile() && full.endsWith(".md")) {
      files.push(full);
    }
  }
  return files;
}

const files = [
  path.join(repoRoot, "README.md"),
  path.join(repoRoot, "CONTRIBUTING.md"),
  path.join(repoRoot, "deploy/README.md"),
  ...walk(path.join(repoRoot, "docs")),
];

const canonicalDocs = fs
  .readdirSync(path.join(repoRoot, "docs"), { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
  .map((entry) => path.join(repoRoot, "docs", entry.name));

const errors = [];

function rel(filePath) {
  return path.relative(repoRoot, filePath).replaceAll(path.sep, "/");
}

for (const filePath of files) {
  const text = fs.readFileSync(filePath, "utf8");
  const linkRegex = /\[[^\]]+\]\(([^)]+)\)/g;
  let match;
  while ((match = linkRegex.exec(text)) !== null) {
    const rawTarget = match[1].trim();
    if (
      rawTarget.startsWith("http://") ||
      rawTarget.startsWith("https://") ||
      rawTarget.startsWith("mailto:") ||
      rawTarget.startsWith("#")
    ) {
      continue;
    }
    const target = rawTarget.split("#")[0];
    if (!target) {
      continue;
    }
    const resolved = path.resolve(path.dirname(filePath), target);
    if (!fs.existsSync(resolved)) {
      errors.push(`${rel(filePath)} -> missing link target ${rawTarget}`);
    }
  }
}

for (const filePath of canonicalDocs) {
  const text = fs.readFileSync(filePath, "utf8");
  if (!/^`Owns`:/m.test(text)) {
    errors.push(`${rel(filePath)} -> missing \`Owns\` header`);
  }
  if (!/^`Depends on`:/m.test(text)) {
    errors.push(`${rel(filePath)} -> missing \`Depends on\` header`);
  }
  if (!/^`See also`:/m.test(text)) {
    errors.push(`${rel(filePath)} -> missing \`See also\` header`);
  }
  if (crossCuttingPages.has(rel(filePath)) && !/^## What This Page Does Not Own$/m.test(text)) {
    errors.push(`${rel(filePath)} -> missing "What This Page Does Not Own" section`);
  }
}

if (errors.length > 0) {
  console.error("docs-check failed:");
  for (const error of errors) {
    console.error(`- ${error}`);
  }
  process.exit(1);
}

console.log(`docs-check passed for ${files.length} markdown files.`);
NODE
