# Getting Started

`See also`: [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md), [MCP Client Setup](mcp/mcp-client-setup.md), [MCP Servers](configuration/mcp-servers.md), [Service Accounts](access/service-accounts.md), [Deploy and Operations](setup/deploy-and-operations.md), [Configuration Reference](configuration/configuration-reference.md)

This page is the admin, user, caller, and client map for the gateway.

- Use it when the behavior spans more than one file.
- Use the owning page instead of chasing the same rule through several docs.
- Use the `Contributing & Internal` top navigation for maintainer workflows, release mechanics, contract generation, migration authoring, and implementation notes.

## Running The Gateway

- Local access, bootstrap admin, and control-plane access: [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md)
- Deployment Artifacts, Database Migrations and general Operations: [Deploy and Operations](setup/deploy-and-operations.md)
- YAML shape, auth modes, provider fields, and config limits: [Configuration Reference](configuration/configuration-reference.md)
- Budgets, spend windows, alerts, and reporting: [Budgets](access/budgets.md)

### MCP
- Server registration and upstream auth modes: [MCP Servers](configuration/mcp-servers.md)
- Toolsets, grants, and effective access: [MCP Tool Access](mcp/mcp-tool-access.md)
- MCP client connection examples: [MCP Client Setup](mcp/mcp-client-setup.md)

### Identity and Access

- Identity lifecycle, team rules, and admin access: [Identity and Access](access/identity-and-access.md)
- Service-account credentials: [Service Accounts](access/service-accounts.md)


## Maintaining The Platform

- Action-oriented recovery and upgrade work: [Admin Runbooks](operations/operator-runbooks.md)
- Request-log payload policy and retention purge: [Observability and Request Logs](operations/observability-and-request-logs.md)
- Cross-cutting request path across routing, logging, pricing, and spend: [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- Provider API family support, OpenAI-compatible route quirks, and compatibility follow-up work: [Provider API Compatibility](reference/provider-api-compatibility.md)
- Admin UI capability map and live surface maturity: [Admin Control Plane](access/admin-control-plane.md)

## Sections

### Setup

- [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md)
- [Deploy and Operations](setup/deploy-and-operations.md)
- [MCP Client Setup](mcp/mcp-client-setup.md)

### Configuration

- [Configuration Reference](configuration/configuration-reference.md)
- [Model Routing and API Behavior](configuration/model-routing-and-api-behavior.md)
- [Pricing Catalog and Accounting](configuration/pricing-catalog-and-accounting.md)
- [MCP Servers](configuration/mcp-servers.md)
- [MCP Tool Access](mcp/mcp-tool-access.md)

### Providers

- [Google Cloud Run OpenAI-Compatible Models](providers/gcp-cloud-run-openai-compat.md)
- [Google Vertex AI](providers/gcp-vertex.md)
- [AWS Bedrock](providers/aws-bedrock.md)

### Operations

- [Observability and Request Logs](operations/observability-and-request-logs.md)
- [Admin Runbooks](operations/operator-runbooks.md)

### Access

- [Identity and Access](access/identity-and-access.md)
- [Service Accounts](access/service-accounts.md)
- [Budgets](access/budgets.md)
- [OIDC and SSO](access/oidc-and-sso-status.md)
- [Admin Control Plane](access/admin-control-plane.md)

### Reference

- [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- [Provider API Compatibility](reference/provider-api-compatibility.md)

## Common Questions

- Model shows up but fails:
  - [Model Routing and API Behavior](configuration/model-routing-and-api-behavior.md)
  - [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- Request succeeds but is not charged:
  - [Pricing Catalog and Accounting](configuration/pricing-catalog-and-accounting.md)
  - [Budgets](access/budgets.md)
  - [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- Compose boot finishes but admin access is unclear:
  - [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md)
  - [Deploy and Operations](setup/deploy-and-operations.md)
  - [Admin Runbooks](operations/operator-runbooks.md)
- MCP client cannot connect:
  - [MCP Client Setup](mcp/mcp-client-setup.md)
  - [MCP Servers](configuration/mcp-servers.md)
  - [MCP Tool Access](mcp/mcp-tool-access.md)
  - [Identity and Access](access/identity-and-access.md)
- MCP client connects but sees no tools:
  - [MCP Tool Access](mcp/mcp-tool-access.md)
  - [MCP Servers](configuration/mcp-servers.md)
  - [MCP Client Setup](mcp/mcp-client-setup.md)
