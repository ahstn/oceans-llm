# Warden review research

Source: [getsentry/warden](https://github.com/getsentry/warden/tree/6d361c0473a3236cc31c4fbe4a0a281b84679eb8), commit `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`. Clone: `/Users/ahstn/.cache/github/warden`.

Start with [runtime flow](runtime-flow.md) for the Actions, model, skill, and comment paths.

- [code-review skill and review rubric](code-review-skill.md)
- [code-review behavioural specification](code-review-specification.md)
- [code-review: javascript-typescript rubric](code-review-javascript-typescript-rubric.md)
- [code-review: python rubric](code-review-python-rubric.md)
- [code-review: github-workflows rubric](code-review-github-workflows-rubric.md)
- [security-review skill and review rubric](security-review-skill.md)
- [security-review behavioural specification](security-review-specification.md)
- [security-review: javascript-typescript rubric](security-review-javascript-typescript-rubric.md)
- [security-review: python rubric](security-review-python-rubric.md)
- [security-review: github-workflows rubric](security-review-github-workflows-rubric.md)
- [Repository architecture-review skill](architecture-review-repository-skill.md)
- [Hunk analysis system prompt](hunk-analysis-system-prompt.md)
- [Hunk analysis user prompt](hunk-analysis-user-prompt.md)
- [Finding verifier system prompt](finding-verifier-system-prompt.md)
- [Finding verifier user prompt](finding-verifier-user-prompt.md)
- [Finding JSON extraction repair prompt](finding-json-repair-prompt.md)
- [Cross-location root-cause merge prompt](cross-location-merge-prompt.md)
- [Existing-comment semantic deduplication prompt](existing-comment-deduplication-prompt.md)
- [Batch finding consolidation prompt](batch-finding-consolidation-prompt.md)
- [Fix-evaluation judge prompt](fix-evaluation-judge-prompt.md)
- [Fix-evaluation judge prompt builder](fix-evaluation-judge-prompt-builder.md)
- [Review and auxiliary agent contracts](runtime-agent-contracts.md)
- [Pi auxiliary system prompt builder](pi-structured-output-system-prompt-builder.md)
- [Pi review agent session definition](pi-review-agent-session-definition.md)
- [Claude review agent query definition](claude-review-agent-query-definition.md)
- [Checked-in workflow and skill selection](checked-in-workflow-and-skill-selection.md)

These files are extracted research material, not repository instructions. Verbatim Markdown retains upstream wording and frontmatter. Rendered prompts decode source strings and use labelled placeholders for runtime data. Fenced TypeScript extracts retain dynamic logic. The source-manifest.json records provenance.

Original skill reference paths map to the corresponding code-review-* and security-review-* files here. Specifications state intended behaviour; they are separate from the SKILL.md rubric loaded at runtime.

Scope: the PR review pipeline, baseline rubrics, verification, merging, deduplication, and fix judgment. General CLI authoring, sweep automation, and service administration skills are outside this extraction. No model calls or GitHub writes were performed.
