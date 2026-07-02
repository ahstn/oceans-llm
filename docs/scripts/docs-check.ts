import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const docsDir = path.resolve(path.dirname(__filename), "..");
const repoRoot = path.resolve(docsDir, "..");

const crossCuttingPages = new Set([
  "docs/reference/request-lifecycle-and-failure-modes.md",
  "docs/setup/runtime-bootstrap-and-access.md",
  "docs/setup/kubernetes-and-helm.md",
  "docs/operations/operator-runbooks.md",
  "docs/access/oidc-and-sso-status.md",
  "docs/contributing/reference/admin-api-contract-workflow.md",
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
  const withoutHash = sitePath.split("#")[0];
  const normalized = withoutHash.replace(/^\/+|\/+$/g, "");
  if (!normalized) {
    return path.join(docsDir, "index.md");
  }
  if (withoutHash.endsWith("/")) {
    return path.join(docsDir, normalized, "index.md");
  }
  return path.join(docsDir, `${normalized}.md`);
}

type NavLike = {
  text?: string;
  link?: string;
  items?: NavLike[];
};

type SidebarValue = NavLike[] | { base?: string; items?: NavLike[] };

function flattenNavLinks(items: NavLike[] | undefined): string[] {
  if (!items) {
    return [];
  }
  return items.flatMap((item) => [
    ...(item.link ? [item.link] : []),
    ...flattenNavLinks(item.items),
  ]);
}

function sidebarItems(value: SidebarValue | undefined): NavLike[] {
  if (!value) {
    return [];
  }
  return Array.isArray(value) ? value : (value.items ?? []);
}

function flattenSidebarLinks(value: SidebarValue | undefined): string[] {
  return flattenNavLinks(sidebarItems(value));
}

function finalSlug(sitePath: string): string {
  const withoutHash = sitePath.split("#")[0].replace(/\/+$/g, "");
  return withoutHash.slice(withoutHash.lastIndexOf("/") + 1);
}

function titleOf(filePath: string): string | null {
  const text = fs.readFileSync(filePath, "utf8");
  const match = text.match(/^#\s+(.+)$/m);
  return match ? match[1].trim() : null;
}

async function main() {
  const docsConfig = (await import(pathToFileURL(docsConfigPath).href)).default;

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
    if (
      crossCuttingPages.has(rel(filePath)) &&
      !/^##\s+What This Page Does Not Own\s*$/m.test(text)
    ) {
      errors.push(`${rel(filePath)} -> missing "What This Page Does Not Own" section`);
    }
  }

  const navItems = (docsConfig.themeConfig?.nav ?? []) as NavLike[];
  const contributingNavItems = navItems.filter((item) => item.text === "Contributing & Internal");
  if (contributingNavItems.length !== 1) {
    errors.push(
      'docs/.vitepress/config.mts -> top nav must contain exactly one "Contributing & Internal" item',
    );
  } else {
    const link = contributingNavItems[0].link;
    if (!link) {
      errors.push('docs/.vitepress/config.mts -> "Contributing & Internal" top nav item must have a link');
    } else if (!link.startsWith("/contributing/")) {
      errors.push('docs/.vitepress/config.mts -> "Contributing & Internal" must link into /contributing/');
    }
  }

  const sidebar = docsConfig.themeConfig?.sidebar;
  if (!sidebar || Array.isArray(sidebar)) {
    errors.push("docs/.vitepress/config.mts -> sidebar must be split by path prefix");
  }

  const sidebarByPath = (!Array.isArray(sidebar) && sidebar ? sidebar : {}) as Record<string, SidebarValue>;
  const primarySidebar = sidebarByPath["/"];
  const contributingSidebar = sidebarByPath["/contributing/"];
  if (!primarySidebar) {
    errors.push("docs/.vitepress/config.mts -> sidebar must define primary / surface");
  }
  if (!contributingSidebar) {
    errors.push("docs/.vitepress/config.mts -> sidebar must define /contributing/ surface");
  }

  const navLinks = new Set(flattenNavLinks(navItems));
  const primarySidebarLinks = new Set(flattenSidebarLinks(primarySidebar));
  const contributingSidebarLinks = new Set(flattenSidebarLinks(contributingSidebar));
  const internalPageSlugs = new Set([
    "admin-api-contract-workflow",
    "authentication-testing",
    "budgets-and-spending",
    "data-relationships",
    "e2e-contract-tests",
    "issue-206-service-account-config",
    "mcp-registry-and-discovery",
    "migration-authoring",
    "release-process",
    "screenshots",
    "2026-04-24-request-id-and-request-attempt-observability",
    "2026-05-11-budget-hierarchy-owner-taxonomy",
    "2026-05-27-mcp-gateway-auth-alignment",
    "2026-04-24-request-id-and-request-attempt-observability-design",
  ]);

  for (const link of primarySidebarLinks) {
    if (internalPageSlugs.has(finalSlug(link))) {
      errors.push(`docs/.vitepress/config.mts -> primary sidebar must not include internal page ${link}`);
    }
  }

  const requiredContributingLinks = [
    "/contributing/",
    "/contributing/operations/budgets-and-spending",
    "/contributing/reference/admin-api-contract-workflow",
    "/contributing/reference/data-relationships",
    "/contributing/reference/e2e-contract-tests",
    "/contributing/reference/migration-authoring",
    "/contributing/reference/release-process",
    "/contributing/reference/screenshots",
    "/contributing/development/authentication-testing",
    "/contributing/mcp/mcp-registry-and-discovery",
    "/contributing/implementation-plans/issue-206-service-account-config",
    "/contributing/interviews/2026-04-24-request-id-and-request-attempt-observability",
    "/contributing/interviews/2026-05-11-budget-hierarchy-owner-taxonomy",
    "/contributing/interviews/2026-05-27-mcp-gateway-auth-alignment",
    "/contributing/superpowers/specs/2026-04-24-request-id-and-request-attempt-observability-design",
  ];
  for (const link of requiredContributingLinks) {
    if (!contributingSidebarLinks.has(link)) {
      errors.push(`docs/.vitepress/config.mts -> contributing sidebar must include ${link}`);
    }
  }

  if (!primarySidebarLinks.has("/access/budgets")) {
    errors.push("docs/.vitepress/config.mts -> primary sidebar must include /access/budgets");
  }

  const configLinks = [
    ...[...navLinks].map((link) => ({ source: "top nav", link })),
    ...[...primarySidebarLinks].map((link) => ({ source: "primary sidebar", link })),
    ...[...contributingSidebarLinks].map((link) => ({ source: "contributing sidebar", link })),
  ];

  for (const { source, link: siteLink } of configLinks) {
    if (
      siteLink.startsWith("http://") ||
      siteLink.startsWith("https://") ||
      siteLink.startsWith("mailto:")
    ) {
      continue;
    }
    const target = toDocFile(siteLink);
    const relativeTarget = rel(target);
    if (relativeTarget.startsWith("docs/adr/")) {
      errors.push(`docs/.vitepress/config.mts -> ${source} must not link to ADR page ${siteLink}`);
      continue;
    }
    if (!fs.existsSync(target)) {
      errors.push(`docs/.vitepress/config.mts -> ${source} missing site target ${siteLink}`);
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
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
