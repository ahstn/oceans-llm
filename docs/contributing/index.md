# Contributing & Internal

`See also`: [Release Process](reference/release-process.md), [Admin API Contract Workflow](reference/admin-api-contract-workflow.md), [Data Relationships](reference/data-relationships.md), [Budgets and Spending](operations/budgets-and-spending.md), [End-to-End Contract Tests](reference/e2e-contract-tests.md)

This surface is for maintainers and contributors changing Oceans LLM itself.

Use the primary docs for admin, user, caller, and client workflows. Use this surface for release work, contract generation, schema and migration changes, implementation plans, interviews, test harnesses, and design notes that help contributors maintain the project.

## Maintainer Workflows

- Release mechanics:
  - [Release Process](reference/release-process.md)
- Database migration authoring:
  - [Migration Authoring](reference/migration-authoring.md)
- Local authentication fixture testing:
  - [Testing Authentication Locally](development/authentication-testing.md)
- Screenshot review assets:
  - [Screenshots](reference/screenshots.md)

## Contracts and Tests

- Admin OpenAPI and generated TypeScript artifacts:
  - [Admin API Contract Workflow](reference/admin-api-contract-workflow.md)
- Browser and HTTP contract harness:
  - [End-to-End Contract Tests](reference/e2e-contract-tests.md)

## Data and Accounting

- Persistent schema and entity relationships:
  - [Data Relationships](reference/data-relationships.md)
- Spend ledger and budget enforcement internals:
  - [Budgets and Spending](operations/budgets-and-spending.md)

## Design Notes

- MCP registry implementation trail:
  - [MCP Registry and Discovery](mcp/mcp-registry-and-discovery.md)
- Active implementation plans:
  - [Issue 206: Top-Level Service Account Config](implementation-plans/issue-206-service-account-config.md)
- Discovery interviews:
  - [Request ID and Request Attempt Observability Interview](interviews/2026-04-24-request-id-and-request-attempt-observability.md)
  - [Budget Hierarchy and Owner Taxonomy Interview](interviews/2026-05-11-budget-hierarchy-owner-taxonomy.md)
  - [MCP Gateway Auth Alignment Interview](interviews/2026-05-27-mcp-gateway-auth-alignment.md)
- Specs:
  - [Request ID and Request Attempt Observability Design](superpowers/specs/2026-04-24-request-id-and-request-attempt-observability-design.md)
