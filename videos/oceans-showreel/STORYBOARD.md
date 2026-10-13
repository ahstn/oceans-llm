---
format: 1920x1080
duration: 26.6s
message: "One governed gateway for every agent, every model — and every dollar."
arc: Hook (many clients, many models) → Converge (one gateway) → Proof flight (route, connect, cap, redact, deny, govern, share) → Reveal (the whole console) → Lockup + CTA
audience: platform and engineering leaders adopting AI coding agents
mode: autonomous
---

## Frame 1 — Converge

- status: animated
- src: compositions/frames/f1-converge.html
- duration: 2.8s
- transition_in: cut
- scene: Agents and providers snap into two arcs, then wire into one gateway
- poster: 2.1s
- blueprint: constellation-hub
- rules: kinetic-beat-slam, vertical-spring-ticker, spring-pop-entrance, svg-path-draw, ambient-glow-bloom, motion-blur-streak (camera push-through)

Cold open on the anaphora that carries the whole film. "Every agent." slams in while six
agent tiles (Claude Code, Codex, OpenCode, Gemini CLI, GitHub Copilot, Pi) spring onto a
left arc; the noun ticks to "model." as six provider tiles (Anthropic, OpenAI, Vertex AI,
Bedrock, OpenRouter, DeepSeek) land on the right arc. The type clears, twelve hairlines draw
inward and pulses race along them into the centre, where the Oceans tile springs up with a
bloom and ripple: "One gateway." The camera then punches into the tile until the frame is
pure ocean blue: the match cut into the flight.

## Frame 2 — Flight

- status: animated
- src: compositions/frames/f2-flight.html
- duration: 20.0s
- transition_in: cut (ocean-blue match)
- scene: A 3D camera flies across five live admin screens; each of seven landings lifts one proof out of the UI
- poster: 1.5s
- blueprint: camera-journey
- rules: 3d-camera-flight, depth-of-field-blur, motion-blur-streak (camera-travel carve-out, whip sweep), nudge-curve, vertical-spring-ticker, waterfall-entry, stat-bars-and-fills, counting-dynamic-scale, hacker-flip-3d, spring-pop-entrance, physics-press-reaction, control-target-sync, press-release-spring, kinetic-beat-slam

The ocean veil lifts as the camera dives onto the first of five real admin captures
floating in a dark 3D void. A persistent bottom-left rail reads "[verb] every [noun]."; the
verb and noun tick on each landing while "every" stays fixed in wave blue.
1. Route every model: the routing plane; a route card lifts out (claude-sonnet-4-6,
   three provider routes with Priority · Weight bars); a weighted-random highlight hops
   and lands.
2. Orbit, then Connect every agent: a cursor clicks the route row's Config button and the
   camera swings round as the routing card flips over to its back face, the generated
   Client config. A scan line reads the JSON and the gateway base URL lights up; the tabs
   page OpenCode → Pi → Claude Code, holding two beats on each (the tab indicator leads,
   the code pages after it and the file path rolls from ~/.config/opencode/opencode.json to
   ~/.pi/agent/models.json to ~/.claude/settings.json), then "Copy JSON" presses and flips
   to "Copied".
3. Whip sweep, then Cap every dollar: the spend-controls plane; Grace's budget counts up to
   $132.00 of $150.00, the bar crosses 80% and the chip flips to "Near limit".
4. Swoop, then Redact every secret: the guardrails plane; a prompt card's fake Anthropic key
   holds long enough to read, then glyph-scrambles into [REDACTED:anthropic-api-key] with a
   "transformed · secret_redaction" decision chip.
5. Pan across the guardrails page, then Deny every delete: the Destructive actions card
   (1.2x the other cards' type scale) lifts out and its mode slides from Audit to Deny. An
   agent types three commands at a readable pace, and each is denied on the score's
   stop-time hits, one every other beat: snow sql DROP DATABASE (database.snowflake),
   op vault delete (secrets.onepassword), aws s3 rb --force (cloud.aws). Each slam stamps
   "Denied", strikes the command through, flashes the row and jolts the lens a little
   harder, while a dolly zoom tightens the perspective around the card. The footer's
   reason code rolls snowflake.drop_database → onepassword.vault_delete →
   aws.destructive_operation, the finished card holds, and the perspective recoils before
   the dive.
6. Dive, then Govern every tool: the MCP invocations plane; a cloud of server tools collapses
   into a four-tool "triage" tool set, and a stray github delete_repository call is denied
   by the saas.github guardrail pack (deny · github.delete_repository). The denied chip
   docks just above the card's top-right corner, fully in frame and slightly enlarged,
   under a slow push-in, and holds for about two seconds before it is flung away.
7. Crane down, tilting over the top of the move, then Share every skill: the Agent Skills
   catalog; incident-triage (platform · Priya Shah) publishes v4: the chip pops in, "Latest"
   slides over and its digit rolls from 3 to 4. Then
   `oceans skills install platform/incident-triage --version 4` types out, the CLI confirms
   "Installed .agents/skills/incident-triage", and the bundle's files land (SKILL.md,
   references/checklist.md, .oceans-skill-lock.json).
The camera then pushes past the last plane into blur: hard cut.

## Frame 3 — Lockup

- status: animated
- src: compositions/frames/f3-lockup.html
- duration: 3.8s
- transition_in: cut (blur resolve)
- scene: The whole console assembles as a tilted wall, then the Oceans LLM lockup lands
- poster: 3.0s
- blueprint: grid-card-assemble, logo-assemble-lockup
- rules: depth-scatter-assemble, depth-of-field-blur, vertical-spring-ticker, spring-pop-entrance, ambient-glow-bloom

Out of the blur, a tilted wall of sixteen real admin panels (request logs, usage and costs,
harness adoption, leaderboard, sessions, MCP invocations, request detail, keys, teams,
batches…) assembles while the camera pulls back; the rail ticks to "See every request." The
lights go down on the wall just ahead of the mark: the Oceans logo lands like a drop, two
water rings splash out and clear, then the mark glides aside as "Oceans LLM" rises letter by
letter from behind its baseline. The rail's "every" flies up into the tagline and the words
ripple out from where it lands: "One governed gateway for every agent and every model." The
oceans-llm.com and "Open source · AGPL-3.0" pills pop in, a sheen crosses the URL, and the
wall keeps drifting as texture through the hold.
