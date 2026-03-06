
# Tooling
Use `mise` for all repo tooling and task execution.

## Mise
- Activate mise in your shell before running tools:
  - `eval "$(/Users/ahstn/.local/bin/mise activate zsh)"`
- Install configured tool versions:
  - `mise install`
- Tool versions and shared tasks live in [`mise.toml`](/Users/ahstn/git/oceans-llm/mise.toml).

## GitHub
- Use `gh` CLI for creating pull requests.
  - For new pull requests, use `.github/PULL_REQUEST_TEMPLATE.md` as the content reference.
- Use `gh` CLI for creating issues.
  - Use `.github/ISSUE_TEMPLATE/feature_request.md` for new features/enhancements (new capability, scoped change, acceptance criteria).
  - Use `.github/ISSUE_TEMPLATE/bug_report.md` for bugs/issues (unexpected behavior, repro details, expected vs actual).

# Documentation Conventions

## Architecture Decision Records (ADRs)
- Record architectural decisions in `./docs/adr/`.
- ADRs should capture the decision, how it is implemented, why that decision was chosen, any trade-offs and follow up items.
