---
workflow: general-video
flow: automation
storyboard: no
message: "One governed gateway for every agent, every model — and every dollar."
aspect: 1920x1080
language: en
length: 22.2s
audience: platform and engineering leaders adopting AI coding agents
---

## Intent

A 22-second, unnarrated motion-design showreel that sells Oceans LLM to new users.
It should feel like a motion designer's résumé piece, "go all out", while every claim
stays true to the product: one governed gateway that routes every agent's traffic to
every model, writes each agent's client config, caps spend, redacts secrets, denies
destructive commands, governs MCP tools, and shares agent skills.

The first cut ran 15 seconds with four flight stops (route, cap, redact, govern). The
extension keeps all four and slots in three more where they belong on the camera path:
client config on the back of the routing card, destructive actions on the guardrails
page beside redaction, and Agent Skills as a closing stop before the lockup.

## Assets

- assets/ui/raw/*.png — 2x captures of the locally seeded admin UI (survey + refined
  dialog sets, captured by scripts/capture-admin.mjs); the 3D flight planes and the
  closing UI wall are cut from these.
- assets/planes/*.jpg — the five flight planes (models, spend-controls,
  guardrails-settings, mcp-invocations, skills), 2400×1350 crops of the seeded captures.
  The models page hosts the routing and client-config stops, the guardrails page hosts
  the redaction and destructive-actions stops, the "Govern every tool" stop uses the
  seeded MCP invocations page, and the "Share every skill" stop uses the seeded Agent
  Skills catalog (assets/ui/raw/skills.png).
- assets/wall/*.jpg — sixteen 1120×630 panels for the closing console wall.
- assets/brand/oceans-icon.png — app-icon logo (from
  ../../docs/public/images/oceans-logo-rounded-square.png) for the hub and the lockup.
- assets/audio/score.wav — the original score, rebuilt by scripts/score.mjs before every
  check, preview and render (gitignored).
- assets/sfx/ — Pixabay sound effects: whooshes, clicks, pops, a glitch and a sparkle.
- assets/fonts/ — Geist Variable latin woff2 (brand display/body face, from
  @fontsource-variable/geist) and JetBrains Mono NL for identifiers.
- ../../crates/admin-ui/web/node_modules/@lobehub/icons-static-svg — monochrome agent and
  provider marks for the opening constellation, inlined as SVG paths.

## Customizations

- Seeded local demo stack (control-oceans-admin, local-demo profile) so the admin pages
  show real-looking traffic, spend, harness adoption and request detail.
- Refined captures that open the routing dialog, the budget-increase dialog, and the
  request-detail sheet so each flight stop lands on one legible detail.
- A seeded Agent Skills catalog on a Skills-enabled local stack
  (OCEANS_VERIFY_SKILLS=true): owners, versions and descriptions for the skills plane.

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
- Audio: an original 150 BPM score synthesized by scripts/score.mjs and cut to the frame
  cues (the deny slams land on its hits), plus Pixabay SFX on one "Sound effects" group.
  The render measures about -15 LUFS integrated with a true peak near -1 dBFS.
- The client-config card mirrors the models page's generated config for OpenCode, Pi and
  Claude Code (file paths, keys and env names as the product writes them), with the
  gateway URL shown as https://gateway.example.com and no real token.
- The destructive-actions card shows the packs and reason codes the guardrails crate
  reports for those exact commands: database.snowflake / snowflake.drop_database,
  secrets.onepassword / onepassword.vault_delete, cloud.aws / aws.destructive_operation.
- The CLI stays pinned to hyperframes@0.8.144 because the user's npm `min-release-age`
  policy blocks newer releases.
