# Bundled skills

This directory holds reviewed skill copies for local ingestion tests and future build packaging. It is outside `.agents/skills/` and other agent auto-load directories. These files are product data, not instructions for agents working on Oceans.

The named Skills verification driver and RustFS end-to-end tests import these files into their disposable demo admin's namespace. Existing local and self-hosted instances use the explicit `mise run skills:import-bundled` task with a user-owned API key. See [local ingestion setup](../docs/setup/skills.md#curated-skills-in-local-testing) for the required environment. This directory does not enable production startup seeding or add skills to a Docker image.

## grill-me

The source is Matt Pocock's [`grill-me`](https://github.com/mattpocock/skills/tree/694fa30311e02c2639942308513555e61ee84a6f/skills/productivity/grill-me), pinned to commit `694fa30311e02c2639942308513555e61ee84a6f` from 2026-06-10. This is the last revision before the skill became a wrapper for the separate `grilling` skill. It works as a single skill without that dependency.

The upstream directory contains only `SKILL.md`. Oceans preserves its instructions and adds `license: MIT`, `metadata.author: Matt Pocock`, and `metadata.github` pointing to the pinned source. Upstream has no version metadata, so no version is added. The repository's original [MIT license](https://github.com/mattpocock/skills/blob/694fa30311e02c2639942308513555e61ee84a6f/LICENSE), including Matt Pocock's copyright notice, is included as `grill-me/LICENSE` without changes.

SHA-256 digests, verified against the pinned GitHub source:

| File | Original source | Bundled copy |
| --- | --- | --- |
| `SKILL.md` | `74147eb6010a65957efef2b9e0f0b3ff935c1def7fc117697151b1d0f3610556` | `93e1db35a9ff91c9826e92697602498a652e1249ae74aed5c07d110f9c4b5e09` |
| `LICENSE` | `0e7ac423bf2c6e223b7c5b156f8cf72da49d748e56a1641402c31f22ad07dbb5` | `0e7ac423bf2c6e223b7c5b156f8cf72da49d748e56a1641402c31f22ad07dbb5` |

Review upstream changes before replacing this copy. Keep the source revision, attribution, license, and digests current when a bundle changes.
