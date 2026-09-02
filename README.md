# Oceans LLM Gateway

<p align="center">
  <img height="400" alt="Oceans LLM logo" src="https://github.com/user-attachments/assets/37d617f1-3eb9-4774-bd38-7b7dd495eab4" />
</p>

Oceans LLM is a policy-aware gateway for routing, governing, and observing AI traffic. It gives clients one API surface while admins manage providers, identities, model access, spend, MCP tools, guardrails, and request observability centrally.

The gateway is written in Rust and includes a same-origin React and TanStack Start admin control plane.

## Find the right documentation

| If you want to... | Start here |
| --- | --- |
| Understand the product and its capabilities | [Overview and Features](docs/overview-features.md) |
| Run Oceans LLM for the first time | [Getting Started](docs/getting-started.md) |
| Choose between local, Compose, and Kubernetes deployments | [Deploy Oceans LLM](docs/setup/deploy-and-operations.md) |
| Understand startup, seeded state, and first access | [Runtime Bootstrap and Access](docs/setup/runtime-bootstrap-and-access.md) |
| Configure providers, models, authentication, and runtime behavior | [Configuration Reference](docs/configuration/configuration-reference.md) |
| Understand routing and supported API families | [Model Routing and APIs](docs/configuration/model-routing-and-api-behavior.md) and [Provider API Compatibility](docs/reference/provider-api-compatibility.md) |
| Configure identities, service accounts, API keys, or budgets | [Identity and Access](docs/access/identity-and-access.md), [Service Accounts](docs/access/service-accounts.md), and [Budgets](docs/access/budgets.md) |
| Register MCP servers and control tool access | [MCP Servers](docs/configuration/mcp-servers.md), [MCP Tool Access](docs/mcp/mcp-tool-access.md), and [MCP Client Setup](docs/mcp/mcp-client-setup.md) |
| Enable and operate gateway guardrails | [Gateway Guardrails](docs/operations/gateway-guardrails.md) |
| Configure telemetry or inspect request logs | [Observability and Request Logs](docs/operations/observability-and-request-logs.md) |
| Upgrade a deployment or recover a failure | [Admin Runbooks](docs/operations/operator-runbooks.md) |
| Contribute to the repository | [Contributing](CONTRIBUTING.md) and [Contributing & Internal](docs/contributing/index.md) |

The [documentation home](docs/index.md) contains the complete user-facing map. Detailed policy belongs in the documentation rather than this README.

## What Oceans LLM provides

### One API surface across providers

Clients can use these gateway endpoints:

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `POST /v1/messages`
- `POST /messages`

Routes can target OpenAI-compatible providers, Google Vertex AI, Amazon Bedrock, Google Cloud Run OpenAI-compatible services, and GitHub Copilot integrations. Support varies by provider and API family. Configure route capability gates so unsupported operations fail at the gateway boundary. See [Provider API Compatibility](docs/reference/provider-api-compatibility.md) for the current matrix.

### Central access and spend controls

Admins can manage:

- Users, teams, service accounts, and scoped API keys
- Model grants and route availability
- User, service-account, and model-specific budgets
- OIDC and OAuth sign-in, including GitHub OAuth
- Pricing provenance, durable usage accounting, and FOCUS-oriented export

### MCP governance

Register Streamable HTTP MCP servers once and expose tools through aggregate or direct routes. Oceans keeps caller credentials separate from upstream MCP credentials and applies tool-set grants before making tools available to users.

MCP invocation logs retain server, tool, owner, policy, latency, and sanitized payload state for investigation.

### Guardrails across model and tool traffic

Configuration-authoritative guardrails can audit or deny prompts, model responses, generated tool calls, MCP calls, MCP results, and supported local harness tool execution. Policies can combine built-in deterministic packs with Amazon Bedrock Guardrails or Google Cloud Model Armor.

Start in audit mode and move individual routes or MCP servers to deny mode after reviewing decisions. See [Gateway Guardrails](docs/operations/gateway-guardrails.md).

### Operational visibility

The admin control plane provides request logs, provider attempts, usage and cost reporting, MCP invocation history, guardrail decisions, agent-session analysis, and coding-harness adoption data.

The gateway exports traces and metrics through OTLP. Deployment artifacts provide hooks for an existing collector or vendor agent but do not install one by default.

## Run the local development stack

### Prerequisites

- [mise](https://mise.jdx.dev/)
- Repository access and the platform tools required by `mise.toml`

Install the pinned toolchain and UI dependencies:

```bash
mise install
mise run ui-install
```

Start the gateway and admin UI:

```bash
mise run dev-stack
```

The local environment uses:

| Component | Default |
| --- | --- |
| Gateway API | `http://localhost:8080` |
| Admin control plane | `http://localhost:8080/admin` |
| Configuration | `gateway.yaml` |
| Database | Local LibSQL or SQLite |
| Bootstrap admin | `admin@local` / `admin` |

When `gateway.db` does not exist, the development stack seeds local demonstration identities, credentials, budgets, request history, and agent-session diagnostics. These credentials and data are for local development only.

Check that the gateway is ready:

```bash
curl --fail http://localhost:8080/healthz
curl --fail http://localhost:8080/readyz
```

Open `http://localhost:8080/admin` to inspect the seeded environment. To recreate the complete local dataset later, run:

```bash
mise run gateway-reset-local-demo
```

This reset command deletes and recreates the local `gateway.db`. Do not use it against data you need to retain.

## Deploy Oceans LLM

Oceans supports these runtime shapes:

- Local development with LibSQL or SQLite
- A production-shaped local stack with PostgreSQL
- Docker Compose with gateway, admin UI, and PostgreSQL containers
- Kubernetes with the published OCI Helm chart and external PostgreSQL or optional CloudNativePG

All public traffic must enter through the gateway. The admin UI runs separately, but the gateway proxies `/admin*` to it so the control plane remains same-origin.

Start with [Deploy Oceans LLM](docs/setup/deploy-and-operations.md). It covers prerequisites, architecture support, secrets, deployment selection, health checks, and rollback planning.

## Common development commands

Run repository tooling through `mise`.

| Task | Command |
| --- | --- |
| Start the local development stack | `mise run dev-stack` |
| Start the production-shaped local stack | `mise run prod-stack` |
| Recreate local demo data | `mise run gateway-reset-local-demo` |
| Check Rust formatting, lint, and tests | `mise run rust-check` |
| Run repository linting | `mise run lint` |
| Run repository unit tests | `mise run test` |
| Build the gateway and admin UI | `mise run build` |
| Run end-to-end contract tests | `mise run e2e-test` |
| Build the documentation | `mise run //docs:build` |
| Lint and render the Helm chart | `mise run helm-check` |

Use `mise tasks` for the complete task list. Maintainer workflows, contract generation, migrations, and release procedures are documented under [Contributing & Internal](docs/contributing/index.md).

## Repository structure

| Path | Responsibility |
| --- | --- |
| `crates/gateway` | HTTP runtime, configuration loading, API handlers, and integration boundaries |
| `crates/gateway-core` | Shared domain types, traits, API DTOs, and errors |
| `crates/gateway-service` | Authentication, model resolution, routing, accounting, and request logging |
| `crates/gateway-providers` | Provider adapters and transport behavior |
| `crates/gateway-mcp` | MCP protocol, discovery, tool normalization, and schema handling |
| `crates/gateway-guardrails` | Guardrail policy resolution, deterministic packs, and managed checks |
| `crates/gateway-store` | LibSQL, SQLite, and PostgreSQL persistence and migrations |
| `crates/admin-ui` | Gateway integration for the admin control plane |
| `crates/admin-ui/web` | React and TanStack Start admin UI |
| `deploy` | Docker Compose and Helm deployment artifacts |
| `docs` | User-facing and contributor documentation |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before making changes. Repository-specific tooling, validation, and structural guidance are recorded in [AGENTS.md](AGENTS.md).
