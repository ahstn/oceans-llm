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
