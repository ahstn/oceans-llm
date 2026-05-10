
# Tooling
Use `mise` for all repo tooling and task execution.

## Mise
- Activate mise in your shell before running tools:
  - `eval "$(/Users/ahstn/.local/bin/mise activate zsh)"`
- Install configured tool versions:
  - `mise install`
- Tool versions and shared tasks live in [`mise.toml`](/Users/ahstn/git/oceans-llm/mise.toml).
- When code changes are introduced, run linting before handing work off:
  - `mise run lint` for mixed Rust/UI changes.
  - `cargo clippy --workspace --all-targets -- -D warnings` if only Rust changed.

## GitHub
- Use `gh` CLI for creating pull requests.
  - For new pull requests, use `.github/PULL_REQUEST_TEMPLATE.md` as the content reference.
- Use `gh` CLI for creating issues.
  - Use `.github/ISSUE_TEMPLATE/feature_request.md` for new features/enhancements (new capability, scoped change, acceptance criteria).
  - Use `.github/ISSUE_TEMPLATE/bug_report.md` for bugs/issues (unexpected behavior, repro details, expected vs actual).

# Structural Code Guidelines

Use line counts as review triggers, not as targets. The goal is code that is easy
to read, test, modify, and review without forcing readers through unnecessary
jumps between tiny helpers.

## Function Size
- Prefer functions under 60 executable lines.
- Functions over 80 executable lines should be easy to scan and justify.
- Functions over 120 executable lines require a reason in review.
- Functions over 150 executable lines should usually be split.
- Functions over 200 executable lines are exceptional.

A long function may be acceptable when:
- It is linear and low-branching.
- It keeps a single coherent workflow together.
- Splitting would create navigation-heavy helper chains.
- The repeated structure is easier to audit in one place.
- It is generated, table-like, parser/state-machine code, or simple dispatch.

A shorter function is not automatically better. Do not split solely to reduce line
count if the result forces readers through several private helpers that are only
meaningful in sequence.

## File and Module Size
- Prefer files in the 200-600 line range.
- Start questioning files around 800 lines.
- Treat 1000 lines as a soft cap.
- Files over 1500 lines need clear justification or a split plan.
- Avoid both mega-files and tiny-file scatter.

Split files and modules by cohesion and ownership, not by arbitrary symbol count.
Related helper types, local utilities, and tightly coupled implementation details
may stay together when that makes the code easier to understand. Split when a
file contains distinct concepts, unrelated responsibilities, or sections that
different maintainers would naturally own independently.

## Review Heuristics
Before requesting a split, ask:
- Can the function's job be summarized in one sentence?
- Is the control flow mostly linear?
- Are there more than 2-3 nesting levels?
- Is cyclomatic complexity above 10 or cognitive complexity above 15-25?
- Would a meaningful sub-part benefit from independent unit tests?
- Would extracting a helper name a real concept, or just hide a few lines?
- Does splitting reduce mental load, or create jump-chasing?

# Documentation Conventions

## Architecture Decision Records (ADRs)
- Record architectural decisions in `./docs/adr/`.
- ADRs should capture the decision, how it is implemented, why that decision was chosen, any trade-offs and follow up items.
