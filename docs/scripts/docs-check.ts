import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const docsDir = path.resolve(path.dirname(__filename), "..");
const repoRoot = path.resolve(docsDir, "..");

const crossCuttingPages = new Set([
  "docs/reference/request-lifecycle-and-failure-modes.md",
  "docs/setup/runtime-bootstrap-and-access.md",
  "docs/operations/operator-runbooks.md",
  "docs/access/oidc-and-sso-status.md",
  "docs/reference/admin-api-contract-workflow.md",
]);

const docsConfigPath = path.join(docsDir, ".vitepress/config.mts");

function rel(filePath: string): string {
  return path.relative(repoRoot, filePath).replaceAll(path.sep, "/");
}

function shouldSkipMarkdown(filePath: string): boolean {
  const relative = rel(filePath);
  return (
    relative.startsWith("docs/adr/") ||
    relative.startsWith("docs/internal/") ||
    relative.startsWith("docs/node_modules/") ||
    relative.startsWith("docs/.vitepress/") ||
    relative === "docs/AGENTS.md" ||
    relative === "docs/README.md"
  );
}

function walk(dir: string): string[] {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files: string[] = [];
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

function toDocFile(sitePath: string): string {
  const normalized = sitePath.replace(/^\/+|\/+$/g, "");
  if (!normalized) {
    return path.join(docsDir, "index.md");
  }
  return path.join(docsDir, `${normalized}.md`);
}

function extractSiteLinks(configText: string): string[] {
  const links: string[] = [];
  const regex = /link:\s*["']([^"']+)["']/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(configText)) !== null) {
    links.push(match[1]);
  }
  return links;
}

function titleOf(filePath: string): string | null {
  const text = fs.readFileSync(filePath, "utf8");
  const match = text.match(/^#\s+(.+)$/m);
  return match ? match[1].trim() : null;
}

const files = [
  path.join(repoRoot, "README.md"),
  path.join(repoRoot, "CONTRIBUTING.md"),
  path.join(repoRoot, "deploy/README.md"),
  ...walk(docsDir).filter((filePath) => !shouldSkipMarkdown(filePath)),
];

const canonicalDocs = files.filter((filePath) => {
  const relative = rel(filePath);
  return relative.startsWith("docs/") && relative !== "docs/index.md";
});

const errors: string[] = [];

for (const filePath of files) {
  const text = fs.readFileSync(filePath, "utf8");
  const linkRegex = /\[[^\]]+\]\(([^)]+)\)/g;
  let match: RegExpExecArray | null;
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
  if (!/^`See also`:/m.test(text)) {
    errors.push(`${rel(filePath)} -> missing \`See also\` header`);
  }
  const seeAlsoMatch = text.match(/^`See also`:(.+)$/m);
  if (seeAlsoMatch) {
    const linkRegex = /\[([^\]]+)\]\(([^)]+)\)/g;
    let match: RegExpExecArray | null;
    while ((match = linkRegex.exec(seeAlsoMatch[1])) !== null) {
      const label = match[1].trim();
      const target = match[2].trim();
      if (!target.endsWith(".md")) {
        errors.push(`${rel(filePath)} -> \`See also\` must only link to markdown files`);
        continue;
      }
      const resolved = path.resolve(path.dirname(filePath), target);
      const expectedTitle = fs.existsSync(resolved) ? titleOf(resolved) : null;
      if (expectedTitle && label !== expectedTitle) {
        errors.push(
          `${rel(filePath)} -> \`See also\` label "${label}" does not match destination title "${expectedTitle}"`,
        );
      }
    }
  }
  if (crossCuttingPages.has(rel(filePath)) && !/^## What This Page Does Not Own$/m.test(text)) {
    errors.push(`${rel(filePath)} -> missing "What This Page Does Not Own" section`);
  }
}

const configText = fs.readFileSync(docsConfigPath, "utf8");
for (const siteLink of extractSiteLinks(configText)) {
  const target = toDocFile(siteLink);
  const relativeTarget = rel(target);
  if (relativeTarget.startsWith("docs/adr/")) {
    errors.push(`docs/.vitepress/config.mts -> sidebar/nav must not link to ADR page ${siteLink}`);
    continue;
  }
  if (!fs.existsSync(target)) {
    errors.push(`docs/.vitepress/config.mts -> missing site target ${siteLink}`);
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
