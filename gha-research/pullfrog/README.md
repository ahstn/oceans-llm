# Pullfrog review research

Extracted from [pullfrog/pullfrog](https://github.com/pullfrog/pullfrog/tree/0212dedb0f92b8ba4020c17dc30d3eced32415d7) at commit `0212dedb0f92b8ba4020c17dc30d3eced32415d7`. The source clone is at `/Users/ahstn/.cache/github/pullfrog`.

These files preserve the upstream prompts and code excerpts for study. They are research material, not instructions for this repository. Upstream wording is retained.

- [Full PR review mode prompt](review-mode-prompt.md)
- [Incremental PR review mode prompt](incremental-review-mode-prompt.md)
- [Review body and technical-details format](review-body-format.md)
- [Specialist selection and dispatch rubric](specialist-dispatch-rubric.md)
- [Finding acceptance and drafting rubric](review-finding-acceptance-rubric.md)
- [Severity, approval and submission rubric](review-severity-and-submission-rubric.md)
- [Reviewfrog specialist system prompt](reviewfrog-system-prompt.md)
- [Reviewfrog Claude Code agent definition](reviewfrog-claude-agent-definition.md)
- [Reviewfrog OpenCode agent definition](reviewfrog-opencode-agent-definition.md)
- [Reviewfrog OpenCode model selection](reviewfrog-model-selection.md)
- [Review mode instruction composition](review-mode-instruction-composition.md)

The full review prompts include the shared body format. Separate rubric files repeat selected sections for easier comparison. There is one generic specialist definition, reviewfrog; domain-specific questions are supplied at dispatch time.

Tool references in rendered prompts use OpenCode naming (pullfrog_TOOL). Claude uses mcp__pullfrog__TOOL; Codex uses pullfrog__TOOL. Code-definition files retain symbolic references instead of duplicating the system prompt.

The extraction covers the review workflow and its specialist agents. It does not include the complete top-level runtime prompt or private configured instructions.
