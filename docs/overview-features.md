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

#### Deployment and Operations

- **Deployment-shaped operations**: Run locally with lightweight storage, deploy with PostgreSQL and Helm, configure secrets through environment references, and export OTLP traces and metrics to existing observability stacks.

## MCP Gateway Features

Oceans sits between MCP clients and the MCP servers they use. Admins register a server once and decide who can use which tools. Every call then goes through the identity, rate limits, guardrails and logs that already govern model traffic. Clients only ever see the tools they're allowed to call.

#### Endpoints and Discovery

- **Central registry and discovery**: Register Streamable HTTP MCP servers once. Oceans discovers each server's tools and keeps their title, annotations and output schema, so clients get the full definition the server declared. See [MCP Servers](configuration/mcp-servers.md).

- **Aggregate `/mcp` endpoint**: One connection covers every server a client can use. Instead of loading every tool definition up front, clients call `search_tools`, `describe_tool` and `call_tool` over a catalog of their granted tools. This keeps tool lists, and context windows, small. Per-tool token-overhead estimates show what each definition costs in context.

- **Direct per-server proxy**: `/mcp/{server_key}` passes a single server's own tools through for clients that want them. The gateway classifies every JSON-RPC method before it reaches the server: tools are filtered and checked one by one, resources, prompts and completions need a grant on the server, and unknown methods are denied.

#### Access Control

- **Per-tool grants and toolsets**: Grant single tools or toolsets to an API key, user, service account or team. Users inherit their team's grants. Toolsets break large servers into smaller sets, so clients aren't overloaded with tools they don't need. See [MCP Tool Access](mcp/mcp-tool-access.md).

- **Tag rules and annotation overrides**: Grant or deny tools by what they do, such as "every read-only tool on this server" for a team, or "never a destructive tool" for a service account. Exclude rules always win, and tools that discovery adds later match without a new grant. Annotations are claims made by the upstream server, so admins can override them per tool and add their own tags, such as `finance` or `pii:low`.

- **Scoped API keys**: Limit a key to MCP, to the aggregate or per-server routes, and to a list of servers. A leaked MCP client key can't call models or spend budget. See [Scoped API keys](mcp/mcp-client-setup.md#scoped-api-keys).

- **Team-owned servers**: Team owners and admins can register their team's servers and grant their tools inside the team, without waiting on a platform admin. Every server has an owner and a contact, and stays visible only to its team until it's published to the organisation. See [Ownership and Visibility](configuration/mcp-servers.md#ownership-and-visibility).

#### Identity and Credentials

- **OAuth sign-in for MCP clients**: Clients that follow the MCP authorization spec connect with just the MCP URL, with no API key to paste. Users sign in the usual way (password, OIDC or GitHub), approve the client on a consent page, and the client gets short-lived tokens tied to that user. Signing out or deactivating the user ends that access, and users can see and disconnect the clients they've authorised. See [OAuth sign-in](mcp/mcp-client-setup.md#oauth-sign-in).

- **Separate upstream credentials**: Oceans API keys never reach MCP servers. Upstream credentials are stored encrypted and bound to the gateway, a user, a service account or a team.

- **Per-user OAuth to any MCP server**: Users connect their own accounts to servers that follow the MCP authorization spec, so tool calls run as them rather than as a shared account. Oceans discovers the server's authorization server, registers itself as a client, and stores and refreshes each user's tokens. URLs that someone other than an admin chose are fetched through a guard that blocks private and internal addresses. See [OAuth for Any MCP Server](configuration/mcp-servers.md#oauth-for-any-mcp-server).

- **Connect from the error**: When a call fails because a user hasn't connected a server, or their connection has expired, the error includes a link that takes them straight to connecting it. See [Connecting from an error](mcp/mcp-client-setup.md#connecting-from-an-error).

#### Safety and Change Control

- **Guardrails on tool calls and results**: Check tool-call arguments before any upstream credential is looked up, and check results before they reach the client. That includes streamed (SSE) results and resource and prompt reads. Secret redaction and built-in packs for destructive actions run first, followed by any managed checks. See [Gateway Guardrails](operations/gateway-guardrails.md).

- **Schema drift detection**: Every discovered tool definition is hashed. A client can send the hash it read with `call_tool`, and the gateway refuses the call if the tool has changed since.

- **Tool versions and approval**: Every definition a server serves for a tool is kept as a version. With approval turned on, new tools and changed tools wait for an admin to review the change, and the last approved definition keeps being served. A server can't quietly change a tool's description or schema after it was reviewed. See [Tool versions and approval](mcp/mcp-tool-access.md#tool-versions-and-approval).

#### Traffic and Audit

- **Rate limits and quotas**: Cap tool calls per minute, hour, day or month, globally or per key, user or team, and per server. The same policies cover model traffic. Responses carry `X-RateLimit-*` headers, and rejected calls also get `Retry-After`. See [Rate Limits](configuration/rate-limits.md).

- **MCP invocation logs**: Every governed call is logged with its method and target, owner context, server and tool, tool version, policy result, latency and status, with redacted and truncated payloads. See [MCP Invocations](mcp/mcp-invocations.md).

- **Admin audit log**: Changes to servers, toolsets, grants, tag rules, annotation overrides, tool approvals, credentials and rate limits record who made them, what changed and where the request came from. See [Admin Audit Log](access/admin-audit-log.md).

- **One gateway for models and tools**: MCP traffic uses the same users, teams, service accounts, API keys, rate limits and guardrails as model traffic. Tool invocations sit alongside request logs and spend budgets in one admin UI.
