# Gateway Configuration Module Boundaries

## Status

Accepted.

## Context

Gateway configuration types, validation, runtime projection, seed projection, and tests were stored in one Rust source file. The file exceeded 6,600 lines and contained several distinct responsibilities. This made changes difficult to review and made related tests hard to find.

The public `gateway::config` path is used across the workspace. A module split must keep that interface and must not change validation order or configuration behavior.

## Decision

`gateway::config` remains the public facade. It defines `GatewayConfig` and re-exports the existing public configuration types.

Implementation files follow configuration responsibilities:

- server, database, authentication, MCP, permissions, logging, alerts, and budgets own their configuration types and validation;
- providers, models, and routes own provider and routing rules;
- identity owns teams, users, and service accounts;
- seeding and runtime own projections from configuration into downstream types;
- references owns environment, path, and secret reference resolution;
- normalization owns the email, team-key, and entity-tag rules shared by more than one domain.

Implementation files import their dependencies explicitly. The facade does not act as a shared prelude for child modules.

Tests use the same behavior-based boundaries. A repository task measures line coverage for production files in the config module and requires at least 90 percent.

## Trade-Offs

- Readers must sometimes move between the facade and one implementation file. Each file now has one clear responsibility, which limits this cost.
- Public re-exports add a small maintenance step when a new public type is added. They preserve the existing API path for callers.
- The coverage task adds work when it runs locally or in CI, but it gives a repeatable regression baseline for future refactors.

## Follow-Ups

- Keep new configuration types and tests with the behavior that owns them.
- Reassess a module when it approaches the file-size review triggers in `AGENTS.md`.
