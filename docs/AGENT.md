# This Nimbus docs site

> `CLAUDE.md` delegates here. Keep project instructions canonical in this file.

Astro-based docs. The `nimbus-docs` package handles content schemas, sidebar/TOC, MDX→markdown, build hooks, and the `nimbus-docs` CLI. Install dependencies first, then run it as `npx @cloudflare/nimbus-docs <command>`: npx uses the project's installed version. Before an install, npx fetches the latest release instead, which may differ from the site's version and give different results. The scoped name keeps npx from fetching the unrelated `nimbus-docs` package. Everything in `src/` is yours to edit.

## File layout

```
astro.config.ts              # imports nimbus + defineNimbusConfig
nimbus.json                  # records the last reviewed Nimbus package version
src/
├── api/openapi.json         # the OpenAPI spec the API reference is generated from
├── assets/landing/          # landing page art and screenshot crops (WebP, optimised at build)
├── components.ts            # MDX globals — components every .mdx can use without an import
├── components/              # AgentDirective, Header, Logo (Oceans mark + wordmark), Render + ui/<slug>/
├── content/
│   ├── docs/*.mdx
│   └── partials/*.mdx       # referenced via <Render file="..." />
├── content.config.ts        # registers docsCollection() + partialsCollection() + apiCollection()
├── landing/                 # the landing page: content.ts (all copy and data), one .astro per section, landing.css
├── layouts/                 # BaseLayout (NimbusHead), DocsLayout (sidebar/TOC/breadcrumbs)
├── lib/cn.ts                # Tailwind className merger
├── pages/
│   ├── index.astro             # the landing page, at /
│   ├── index.md.ts             # the landing page as Markdown, built from landing/content.ts
│   ├── [...slug].astro
│   ├── api/[...slug].astro     # the API reference, at /api
│   ├── [...slug]/index.md.ts   # Markdown version of every page, all collections
│   ├── [...slug]/index.mdx.ts  # authored source of every page, all collections
│   ├── [...section]/llms.txt.ts  # per-section llms.txt, nested sections included
│   ├── 404.astro
│   ├── llms.txt.ts
│   ├── llms-full.txt.ts
│   ├── nimbus-api/coordinates.json.ts  # API citation manifest
│   ├── og.png.ts                # site-level OG card
│   ├── og/
│   │   ├── _card.ts             # the card's look, shared by both routes (underscore = not a route)
│   │   ├── _fonts/              # the card's fonts
│   │   └── [...slug].ts         # per-page OG cards
│   └── robots.txt.ts
└── styles/                  # globals.css, prose.css
```

The project root is the repo's `docs/` folder. Beside the site it holds:

- `public/`: files served as is. `public/_redirects` sends the old VitePress URLs to the new pages, and `public/images/` and `public/r/` (the shadcn theme registry) keep their old URLs.
- `wrangler.jsonc`: the Cloudflare Pages project and the build output directory.
- `mise.toml`: the `install`, `dev`, `build`, `check`, `preview` and `deploy` tasks. Run them from the repo root as `mise run //docs:<task>`.
- `AGENTS.md`: the writing guide for every page.
- Maintainer documents that aren't published: `adr/`, `contributing/`, `internal/`, `plans/`, `interviews/`, `design-proposals/` and `README.md`. Astro only builds `src/` and copies `public/`, so these stay out of the site. Don't move them into either folder. Link to them from site pages with GitHub URLs (`https://github.com/ahstn/oceans-llm/blob/main/docs/...`).

## Deploying

The site is static. The Cloudflare Pages project `oceans-llm-docs` serves it at `https://oceans-llm.com` and `oceans-llm-docs.pages.dev`. Deploys are manual: CI only builds and checks.

- `mise run //docs:preview` builds the site and serves it with `wrangler pages dev`, which applies `_headers` and `_redirects` like production.
- `mise run //docs:deploy` runs the checks, then uploads `dist/` to production. Set `CLOUDFLARE_PAGES_BRANCH=<name>` for a preview deploy at `<name>.oceans-llm-docs.pages.dev` instead.
- The deploy needs `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` (Pages edit permission). Put them in the gitignored `mise.local.toml` in this folder.
- When you move or rename a page, add a rule to `public/_redirects` for the old URL. Static rules go before the `/contributing/*` splat rule.

## Landing page

`/` (`https://oceans-llm.com/`) is a landing page for visitors, not a docs page. The docs start at `/introduction/` (`src/content/docs/introduction.mdx`), which the header's Docs tab opens. The logo and the breadcrumb's Home link go to the landing page.

- Change copy, links or screenshots in `src/landing/content.ts`. The page and its Markdown version both read it. Link labels follow the docs rule: use the destination page's title.
- Product pictures (the hero, screenshot stages, the trace and code panels) sit in `data-mode="dark"` subtrees, so they stay dark in light mode. `globals.css` resolves the Tailwind colour utilities again in such a subtree.
- Screenshots come from `public/images/`, cropped and saved as WebP in `src/assets/landing/`. The wave art is generated in the style of the admin UI's sign-in illustration. Astro resizes all of them at build time.

## Writing docs

Frontmatter validates against the schema from `docsCollection()` (`@cloudflare/nimbus-docs/content`). Required: `title`.

```mdx
---
title: My page
description: One-line summary.
---

Content here. The page H1 comes from `title` — don't repeat it in the body.

## Section heading
```

Rules:

- **Components are PascalCase.** Register one in `src/components.ts` to use it in every `.mdx` without an import, or import it in the file that uses it. A pre-build validator catches unresolved names with a "did you mean" hint.
- **Partials use `<Render file="..." />`.** Don't import `.mdx` directly. Shared content lives in `src/content/partials/<slug>.mdx`.
- **Icons use Nimbus's `Icon` + Phosphor.** `<Icon name="ph:<glyph>" class="w-4 h-4" />` from `@cloudflare/nimbus-docs/components/Icon.astro`. Glyphs: [phosphoricons.com](https://phosphoricons.com).
- **Don't remove `<AgentDirective />` from `BaseLayout.astro`.** It points agents at `/llms.txt`.

## Adding things

| Goal | Action |
|---|---|
| New doc page | Create `src/content/docs/<slug>.mdx`. Sidebar picks it up. |
| New partial | Create `src/content/partials/<slug>.mdx`. Use via `<Render file="<slug>" />`. |
| UI from registry | `npx @cloudflare/nimbus-docs add <slug>`. Register in `src/components.ts` if used in MDX. |
| Feature recipe | `npx @cloudflare/nimbus-docs add <feature-slug> --print`. Prints the recipe for you to follow; it changes no files itself. |
| Check it builds | `npx @cloudflare/nimbus-docs check` — build-free preflight (env + structure + authoring + types). `--json` for an agent loop, `--fix` to repair what's safe. |
| Custom page route | Add a file under `src/pages/`. |
| Custom OG style | Edit `src/pages/og/_card.ts`. The card draws the Oceans mark from `src/pages/og/_oceans-mark.png` beside the site name, like the header. |
| Change the logo or favicon | The header mark is `public/brand/oceans-mark.png` (96 px) and the favicon is `public/favicon.png` (64 px). Both are downscaled from `public/images/oceans-logo-rounded-square.png`, for example with `sips -Z 96 <source> --out <target>`. Nimbus picks the first of `favicon.svg`, `favicon.ico` and `favicon.png` in `public/`. |
| Check for updates | `npx @cloudflare/nimbus-docs outdated` — starter files behind their tag + registry components behind. |
| Upgrade Nimbus | Update the package, then run `npx @cloudflare/nimbus-docs migrate --dry-run --diff`. Review every change and required manual step before applying. |
| Upgrade a starter file | `npx @cloudflare/nimbus-docs diff <file>` to review, `diff --apply <file>` to pull a clean upstream change. |
| Upgrade a registry component | `npx @cloudflare/nimbus-docs add <slug> --overwrite`, then review with `git diff`. Components you edited are kept and listed. To compare one with upstream, commit, run the `--overwrite --yes` command it prints, and read `git diff`; unless the user decided to discard their edits, undo the whole run with `git checkout -- . && git clean -fd`, since it also rewrote `nimbus.json`. |

Sätteri is Nimbus's default Markdown and MDX processor. Extend it using `markdown.mdastPlugins` for Markdown AST transformations or `markdown.hastPlugins` for HTML AST transformations.
If the site replaces Sätteri with another processor, set `admonitions: false` and keep that processor's existing callout implementation.

List installable items: `npx @cloudflare/nimbus-docs list`.

## Sidebar

The sidebar mirrors `src/content/docs/`. A folder becomes a group, and its `index.mdx` is the group's landing page; set `sidebar.group.label` there to rename the group. Within a group, `sidebar.order` sorts entries (lowest first, otherwise alphabetical), `sidebar.label` shortens a page's rail title, and `sidebar.hidden: true` builds a page without listing it.

To add an external link or fix the top-level order, set `sidebar.items` in `defineNimbusConfig` in `astro.config.ts`. Items you list replace the generated top level, and a listed item's `label` wins over the folder's `sidebar.group.label`:

```ts
sidebar: {
  items: [
    { label: "Guides", autogenerate: { directory: "guides" } },
    { label: "GitHub", link: "https://github.com/you/repo" },
  ],
},
```

The header's section tabs come from the same list: each top-level item that autogenerates from another collection (`{ label: "API", autogenerate: { collection: "api", prefix: "/reference" } }`) becomes a tab, and the tabs show once there are two. With one, an API reference served away from `/` makes the tabs: Docs and API reference. Folders of `src/content/docs/` and external links stay in the sidebar. For any other header link, edit `src/components/Header.astro`.

This site lists every top-level group in `astro.config.ts`, so a new folder doesn't show until you add it there. A folder's `index.mdx` is a normal entry in its group: give it `sidebar.order: 0` to keep it first. Top-level groups set `collapsed: false`, so they open on load. `sidebar.defaultCollapsed` is `true`, so nested groups (a subfolder such as `access/sso/` or `providers/aws-bedrock/`) stay closed unless they hold the current page. A nested group takes its label and position from its `index.mdx`: `sidebar.group.label` and `sidebar.order`.

Page icons in the sidebar come from `src/lib/sidebar-icons.ts`, keyed by page path. The content schema has no per-page icon field. When you add or move a page, add its path there with a Phosphor (`ph:`) icon name; a page that isn't listed shows no icon. Group icons come from Nimbus itself: `icon` on each top-level item in `astro.config.ts`, and `sidebar.group.icon` in a nested group's `index.mdx`. Every row has an icon, so give a new group one that differs from its Overview page's icon.

Everything else: <https://nimbus-docs.com/navigation/sidebar/>.

## API reference

The reference at `/api` is generated from `src/api/openapi.json`, an OpenAPI 3.x spec, through the `api` entry in `astro.config.ts` and the route `src/pages/api/[...slug].astro`. Its pages, Markdown versions, search entries and `llms.txt` follow the spec: change the spec, not the pages.

- Replace the spec: overwrite `src/api/openapi.json`, or point `spec` in the `api` entry at another local file, then build.
- Add a version, link an operation from prose with `api.ref:`, or change the reference's URLs: <https://nimbus-docs.com/api-reference/>.
- Its components in `src/components/ui/` came from the registry; upgrade them like any registry component.

## Agent interfaces

Read the release-matched guide at [`node_modules/@cloudflare/nimbus-docs/dist/docs-for-agents.md`](node_modules/@cloudflare/nimbus-docs/dist/docs-for-agents.md), especially for preview packages. Nimbus publishes discovery automatically; add project-owned Skills under `skills/<name>/SKILL.md`. Keep `<WebMCP />` in `BaseLayout.astro` for browser-agent search with default Pagefind, and render `<ConnectMcp />` in `DocsLayout.astro` only when `capabilities.mcpUrl` is set (`agents: { mcp: true }`; needs a server adapter and `@modelcontextprotocol/server`). Markdown negotiation applies to request-rendered pages; static hosting and response headers follow the deployment target's setup.

## Upgrading Nimbus

Keep `nimbus.json` committed. Its `lastReviewedNimbusVersion` is the baseline Nimbus uses to select the versioned reviews crossed by a package upgrade; state-detected migrations come from the current project files. It is not a package pin and should not be edited by hand.

1. Update `@cloudflare/nimbus-docs` with the project's package manager.
2. Preview the complete plan with `npx @cloudflare/nimbus-docs migrate --dry-run --diff`. If no baseline exists yet, add `--from <previous-version>`.
3. Review every versioned entry and resolve each blocked/manual item.
4. Apply safe edits only with explicit consent: `npx @cloudflare/nimbus-docs migrate --yes`. Review the resulting diff, then rerun the preview.
5. When no migration remains, run `npx @cloudflare/nimbus-docs migrate --yes` again to record the completed review in `nimbus.json`.
6. Pull in starter fixes: `npx @cloudflare/nimbus-docs outdated` lists starter files that changed upstream. For each one, review it with `npx @cloudflare/nimbus-docs diff <file>`, and take a clean update with `diff --apply <file>`. Merge files you've edited by hand. `migrate` doesn't cover these.
7. Run the project's typecheck and production build, then run `npx @cloudflare/nimbus-docs check` again for post-build coverage.

Except for task-printing mode (`--print`), `migrate` exits nonzero while work or review remains; that is a pending-upgrade signal, not necessarily a command failure. Never skip versions by changing `nimbus.json` directly.

## Audit this site

First set `site` in `astro.config.ts` to the production URL; a fresh site still has the placeholder. Ask the user for the URL if you don't know it, and never make one up.

Then run `npx @cloudflare/nimbus-docs check --json`. It runs the environment, structural, authoring, and type checks build-free — config validity, `site` placeholder, route collisions, MDX component resolution, the lint rules, and a `tsc` type-check — and returns three top-level signals plus per-scope detail:

- **`status`** (`passed` | `failed` | `partial`) and **`readiness`** (`buildable` | `blocked` | `unknown`) are the primary signals. `status` is the whole-run verdict; `readiness` answers "does env + structure say it builds?". `ok` (=== zero errors) is kept for back-compat only.
- **`findings[{scope,code,severity,file,line,message,fixable,fix}]`** are problems we evaluated. Apply each `fix` (or `check --fix`).
- **`scopes[].notes[{code,reason,requiresBuild?,requiresInput?}]`** are checks we *couldn't* evaluate yet (e.g. types before a build). A note is never a finding and never carries a `fix` — you resolve it by making the missing thing exist (usually a build), not by `--fix`. `summary.notes` counts them.

Loop until `status !== "failed"` and no finding has a `fix` without `fix.requiresInput` — `summary.fixable` also counts fixes that need input (a placeholder `site`), which `--fix` can't apply for you. An error with no `fix` (such as `nimbus/dependencies-missing`) ends the loop too: do what its message says (install dependencies), then run `check` again, or stop and report it. A `partial` run with nothing left to fix is a **stop** (optionally build, then re-check), not a `--fix` retry. Exit is `1` only when `status` is `"failed"`. For full coverage (types + link-checking) run a build first, then `check` again. With server output, `check` stays `partial` with the note `nimbus/request-rendering-build-required` even after a build: it doesn't verify request-rendered pages, so a passing production build is the gate.

Then walk the categories below for what `check` doesn't cover yet — route-file existence, registry hygiene, the AI surface, post-build search, and Cloudflare config. Emit findings as:

```
- [error|warn|info] FILE:LINE — what + why + fix.
```

End with `Summary: N errors, N warnings.`

- **Config** — `astro.config.ts` calls `nimbus(defineNimbusConfig({ ... }))`; `site` is set; `editPattern` (if set) contains `{path}`; `output:` matches the deploy target.
- **Content** — `content.config.ts` registers `docsCollection()` (and `partialsCollection()` if used); every `.mdx` is inside a registered collection; frontmatter validates.
- **Sidebar** — every sidebar ref resolves to a content entry; no orphans; no slug collisions.
- **MDX** — every PascalCase component in `*.mdx` is registered or imported; every `<Render file=...>` resolves; code-fence languages are valid.
- **Routes** — `llms.txt.ts`, `robots.txt.ts`, `[...slug]/index.md.ts`, `og.png.ts`, `og/[...slug].ts` all exist.
- **Registry hygiene** — every `src/components/ui/<slug>/` is either MDX-registered or imported in `src/`; transitive deps (`lib/cn.ts`, etc.) exist.
- **AI surface** — `<AgentDirective />` renders in `BaseLayout.astro`; doc `<head>` has `<link rel="alternate" type="text/markdown" ...>`.
- **Search** — `data-pagefind-body` is on the docs main wrapper; after a production build, `dist/pagefind/` exists with ≥1 indexed page.
- **Cloudflare** — this site deploys to Cloudflare Pages, not Workers. `wrangler.jsonc` has `name` (the Pages project), `compatibility_date` and `pages_build_output_dir = "./dist"`, and no `assets` or `main`. Pages serves `dist/404.html` for unknown paths and reads `dist/_headers` and `dist/_redirects`.

## Don't

- Hand-add components under `src/components/ui/` that exists in the Nimbus registry — use `npx @cloudflare/nimbus-docs add <slug>` so deps resolve.
- Import `.mdx` files directly — use `<Render file="..." />`.
- Remove `<AgentDirective />` unless asked.

## Project home

[nimbus-docs.com](https://nimbus-docs.com)
