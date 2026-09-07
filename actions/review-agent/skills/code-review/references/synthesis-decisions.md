# Synthesis Decisions

Use this record when maintaining the skill. The five axis files preserve the base skill's review concerns and append evidence and structural checks. Conflicting naming, line-count, reuse, and instruction-authority bullets are normalized in place so the model receives one consistent rule.

## Sources

- Base: `~/git/dotfiles/skills/code-review-and-quality/`, including its five axes and review references. The bundled skill keeps the action's existing `code-review` name and path. Its Codex explicit-invocation policy is retained; the action explicitly selects the skill.
- Structural review: `~/.agents/skills/thermo-nuclear-code-quality-review/SKILL.md`.
- Pullfrog: the fetched prompts and rubrics in `gha-research/pullfrog/`, pinned to `0212dedb0f92b8ba4020c17dc30d3eced32415d7`; especially finding acceptance, specialist dispatch, incremental review, and severity/submission.
- Warden: the fetched prompts and rubrics in `gha-research/warden/`, pinned to `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`; especially code/security review, language/workflow rubrics, finding verification, consolidation, and semantic deduplication.

These are provenance paths, not runtime dependencies. The bundled references contain the guidance required for review.

The subsequent instruction audit used the supplied OpenAI GPT-6 Astra guidance, especially instruction following, autonomous completion, delegation, writing style, and proportionate verification: https://developers.openai.com/api/docs/guides/latest-model. The resulting rules apply to this non-interactive workflow without changing its model or API configuration.

## Conflicts resolved

| Source conflict | Adopted rule and reason |
| --- | --- |
| Base offers local/chat reporting; upstream tools use different schemas. | GitHub comments only. Use the existing action submission schema and publisher; no new report modes, badges, verdict vocabulary, or direct worker writes. |
| Pullfrog delegates by uncertainty with no size threshold; base assigns five parallel agents. | The user's threshold wins: below 80 lines permits one agent; 80 or more requires multiple passes. Combine the two structural axes to fit the runtime's four-child limit. Foreground execution preserves the sandbox policy. |
| Warden's correctness skill excludes other axes. | Retain all five axes. Apply the bug proof bar to correctness, source-to-sink proof to security, and concrete maintenance-cost proof to structural findings. |
| Thermo-nuclear review treats 1000 lines and plausible simplification as presumptive blockers; Warden architecture uses still lower size limits. | Follow project guidance: sizes trigger inspection, not findings. Require demonstrated cost and a feasible remedy; do not split cohesive code to reach a number. |
| Warden's verifier can retain plausible but unproven paths; its main rubric requires proof. | Keep only findings with sufficient code-level evidence. Narrow a proven impact when needed; reject speculation instead of relabeling it low severity. |
| Pullfrog expects broad concerns on substantial PRs. | Investigate rollout, cleanup, and compatibility without requiring a finding. Empty results remain valid. |
| Upstream prompts prescribe thread retirement and automatic approval. | Deduplicate available comments without modifying threads or issuing approval. The host controls review events, and unseen comments are an explicit limit. |
| Base uses Critical/Important/Consider and P1/P2/P3 badges; research uses other labels. | Use the existing low/medium/high/critical schema. Confidence is separate; publisher formatting supplies severity. |
| Thermo-nuclear guidance can invite implementation changes. | This is a read-only review. Suggest a concrete structural remedy, then publish through the host. |
| Base confuses a readability score with a grade level. | Use ASD-STE100 and Flesch-Kincaid Grade Level 7–10, matching this repository's instructions. |

The research's provider choices, tool names, JSON repair prompts, fixed report layouts, and fix-agent workflows are not review criteria and are not imported.

Non-interactive completion is explicit in the action prompt. Caller preferences can override rubric defaults within host permissions; PR-authored files remain evidence. Investigation ends after coverage and candidate verification, with material gaps reported. Instruction-based limits identify the exact source and requirement. Child reports use the same clear writing standard as public comments.
