---
workflow: general-video
flow: automation
storyboard: no
message: "One governed gateway for every agent, every model — and every dollar."
aspect: 1920x1080
language: en
length: 15s
audience: platform and engineering leaders adopting AI coding agents
---

## Intent

A 15-second, unnarrated motion-design showreel that sells Oceans LLM to new users.
It should feel like a motion designer's résumé piece, "go all out", while every claim
stays true to the product: one governed gateway that routes every agent's traffic to
every model, caps spend, redacts secrets, and governs MCP tools.

## Assets

- assets/ui/raw/*.png — 2x captures of the locally seeded admin UI (survey + refined
  dialog sets, captured by scripts/capture-admin.mjs); the 3D flight planes and the
  closing UI wall are cut from these.
- assets/planes/*.jpg — the four flight planes (models, spend-controls,
  guardrails-settings, mcp-invocations), 2400×1350 crops of the seeded captures. The
  "Govern every tool" stop uses the seeded MCP invocations page.
- assets/wall/*.jpg — sixteen 1120×630 panels for the closing console wall.
- assets/brand/oceans-icon.png — app-icon logo (from
  ../../docs/public/images/oceans-logo-rounded-square.png) for the hub and the lockup.
- assets/fonts/ — Geist Variable latin woff2 (brand display/body face, from
  @fontsource-variable/geist) and JetBrains Mono NL for identifiers.
- ../../crates/admin-ui/web/node_modules/@lobehub/icons-static-svg — monochrome agent and
  provider marks for the opening constellation, inlined as SVG paths.

## Customizations

- Seeded local demo stack (control-oceans-admin, local-demo profile) so the admin pages
  show real-looking traffic, spend, harness adoption and request detail.
- Refined captures that open the routing dialog, the budget-increase dialog, and the
  request-detail sheet so each flight stop lands on one legible detail.

## Notes

- DESIGN.md at the repo root is brand truth: ocean #006f9f / wave #0b83ad accents under
  10%, Geist 500 display, dark oklch surfaces, border-first cards, green/amber/red only
  as semantic state, no gradient text, glassmorphism or decorative grids.
- The remote (tailnet) admin URL was not usable for capture (404 on the admin route), so all UI
  comes from the local seeded stack.
- MCP servers were not seeded because they need real upstream credentials; the "Govern
  every tool" stop lands on the seeded MCP invocations capture instead, and its tool-set
  widget is drawn from the documented Tool sets behaviour.
- The redaction widget uses an obviously fake key string; no real credentials appear.
- Budgets are per user / service account (not per team), and the UI calls MCP groupings
  "Tool sets"; widgets follow both.
- CTA: oceans-llm.com, open source (AGPL-3.0).
- No audio in the build; an SFX/music bed is offered after review, not added.
- The CLI stays pinned to hyperframes@0.8.144 because the user's npm `min-release-age`
  policy blocks newer releases.
