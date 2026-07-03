# Overview and Features

`See also`: [Documentation Home](index.md)

## Why Oceans LLM?

Oceans LLM gives teams one governed gateway for AI traffic: route requests across providers, manage identity and service access, control spend, observe usage, and expose MCP tools without asking every client to solve those problems separately.

## Features

#### Access, Identity, and Permissions

- **SSO-ready admin access**: Support for OIDC/OAuth SSO, including GitHub OAuth and configured OIDC providers,.

- **First-class users, teams, and service accounts**: Model all identities separately, with service-account credentials, ownership, model grants, and budgets that remain independent from team membership.

  - Budgets are intentionly dropped for teams, avoiding spend abuse and promoting fairness.

- **Scoped API keys and model access**: Gateway API keys for users or service accounts providing smoother on-boarding, granular model access, and credential revocation.

#### Routing and Client Experience

- **Provider routing behind one API surface**: Let callers use OpenAI-compatible, Anthropic Messages-compatible, Responses, and embeddings endpoints while admins manage aliases, provider routes, capability gates, and compatibility behavior centrally.

- **Server-generated client setup**: Generate client configuration snippets for supported model harnesses so users can connect tools like Claude Code, Codex, Opencode, and Pi with fewer manual setup mistakes.

- **Tags for attribution and reconciliation**: Attach bounded request tags and identity tags so usage can be mapped back to cost centers, workloads, applications, environments, or existing internal company specific metadata.

- **Extensive Spend controls**: Set budgets for users, service accounts, and model specific budgets by entity. Enabling access to frontier, costly models without risking runaway spend and complete loss of service.

#### Observability and Audit

- **Durable usage accounting**: Record request usage into a ledger with pricing provenance, effective-dated rates, and clear `priced`, `unpriced`, and `usage_missing` states instead of relying on approximate billing.

- **FOCUS-oriented FinOps export**: Export best-effort FOCUS-compatible CSV data for LLM usage so FinOps teams can ingest costs into existing tooling without maintaining brittle custom pipelines.

- **Request observability and audit detail**: Inspect request logs with owner context, model resolution, provider attempts, latency, token usage, tags, payload policy, truncation state, and request correlation.

- **Agent harness adoption tracking**: Understand which coding-agent clients, such as Claude Code, Opencode, Pi, Gemini CLI, Copilot CLI, and GitHub Copilot, are sending traffic through the gateway.

#### MCP Governance

- **Central MCP registry and access control**: Register Streamable HTTP MCP servers once, discover tools, and expose either aggregate `/mcp` search/describe/call flows or direct per-server proxy routes.

- **Scoped MCP Tool Sets**: Decompose multiple MCPs into smaller user specific toolsets to avoid tool overload and reduce risk of unintended tool execution.

- **MCP credential separation**: Keep Oceans API keys separate from upstream MCP credentials, with gateway-managed and principal-bound credential bindings for safer tool execution.

- **MCP invocation logs**: Audit individual tool calls with request correlation, owner context, server/tool identity, policy result, latency, status, and sanitized argument/result payload state.

#### Deployment and Operations

- **Deployment-shaped operations**: Run locally with lightweight storage, deploy with PostgreSQL and Helm, configure secrets through environment references, and export OTLP traces and metrics to existing observability stacks.
