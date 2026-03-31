# Oceans LLM Documentation

This site is the operator and maintainer map for the gateway.

- Use it when the behavior spans more than one file.
- Use the owning page instead of chasing the same rule through several docs.
- Keep ADRs in the repo for decision history. They are not part of the public nav in this pass.

## Running The Gateway

- First boot, local access, bootstrap admin, seeded API keys:
  - [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md)
- Runtime shape, same-origin model, local versus deploy caveats:
  - [Deploy and Operations](setup/deploy-and-operations.md)
- YAML shape, auth modes, provider fields, and config limits:
  - [Configuration Reference](configuration/configuration-reference.md)
- Identity lifecycle, team rules, and current admin access overlays:
  - [Identity and Access](access/identity-and-access.md)
- Budgets, spend windows, alerts, and reporting:
  - [Budgets and Spending](operations/budgets-and-spending.md)

## Maintaining The Platform

- Action-oriented recovery and upgrade work:
  - [Operator Runbooks](operations/operator-runbooks.md)
- Cross-cutting request path across routing, logging, pricing, and spend:
  - [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- Admin UI capability map and live versus preview-backed surface split:
  - [Admin Control Plane](access/admin-control-plane.md)
- Generated admin contract, checked-in artifacts, and drift rules:
  - [Admin API Contract Workflow](reference/admin-api-contract-workflow.md)
- E2E harness scope and release-side checks:
  - [End-to-End Contract Tests](reference/e2e-contract-tests.md)
  - [Release Process](reference/release-process.md)

## Sections

### Setup

- [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md)
- [Deploy and Operations](setup/deploy-and-operations.md)

### Configuration

- [Configuration Reference](configuration/configuration-reference.md)
- [Model Routing and API Behavior](configuration/model-routing-and-api-behavior.md)
- [Pricing Catalog and Accounting](configuration/pricing-catalog-and-accounting.md)

### Operations

- [Budgets and Spending](operations/budgets-and-spending.md)
- [Observability and Request Logs](operations/observability-and-request-logs.md)
- [Operator Runbooks](operations/operator-runbooks.md)

### Access

- [Identity and Access](access/identity-and-access.md)
- [OIDC and SSO Status](access/oidc-and-sso-status.md)
- [Admin Control Plane](access/admin-control-plane.md)

### Reference

- [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- [Data Relationships](reference/data-relationships.md)
- [Admin API Contract Workflow](reference/admin-api-contract-workflow.md)
- [End-to-End Contract Tests](reference/e2e-contract-tests.md)
- [Release Process](reference/release-process.md)

## Common Questions

- Model shows up but fails:
  - [Model Routing and API Behavior](configuration/model-routing-and-api-behavior.md)
  - [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- Request succeeds but is not charged:
  - [Pricing Catalog and Accounting](configuration/pricing-catalog-and-accounting.md)
  - [Budgets and Spending](operations/budgets-and-spending.md)
  - [Request Lifecycle and Failure Modes](reference/request-lifecycle-and-failure-modes.md)
- Compose boot finishes but admin access is unclear:
  - [Runtime Bootstrap and Access](setup/runtime-bootstrap-and-access.md)
  - [Deploy and Operations](setup/deploy-and-operations.md)
  - [Operator Runbooks](operations/operator-runbooks.md)
- Live admin contract changed and the UI drifted:
  - [Admin API Contract Workflow](reference/admin-api-contract-workflow.md)
  - [End-to-End Contract Tests](reference/e2e-contract-tests.md)
