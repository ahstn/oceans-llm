# Agent Tooling Guide

Use `mise` for all repo tooling and task execution.

## Setup
- Activate mise in your shell before running tools:
  - `eval "$(/Users/ahstn/.local/bin/mise activate zsh)"`
- Install configured tool versions:
  - `mise install`

## Configuration
- Tool versions and shared tasks live in [`mise.toml`](/Users/ahstn/git/oceans-llm/mise.toml).
- Add new tool dependencies under `[tools]`.
- Add standard team commands under `[tasks.<name>]`.

## Tasks
- Run project commands through mise tasks:
  - `mise run build`
  - `mise run check`
  - `mise run test`
  - `mise run fmt`
  - `mise run lint`
  - `mise run run`

## GitHub
- Use `gh` CLI for creating pull requests.
  - For new pull requests, use `.github/PULL_REQUEST_TEMPLATE.md` as the content reference.
- Use `gh` CLI for creating issues.
  - For new issues, use `.github/ISSUE_TEMPLATE/work-item.md` as the content structure/reference.
