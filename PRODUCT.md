# Product

## Register

product

## Users

Platform admins and operators running an LLM gateway control plane. They work in operational, security-sensitive contexts where access, spending, model routing, MCP/tooling, and request behavior need to be understood and changed without ambiguity.

## Product Purpose

Oceans Gateway Admin is the same-origin control plane for operating the Oceans LLM Gateway. It lets admins manage API keys, identity, teams, service accounts, budgets, spending, request logs, MCP registry/toolsets, and model configuration from one authenticated surface.

Success means operators can inspect current system state, make safe administrative changes quickly, and distinguish live capabilities from maturing areas without leaving the UI or guessing from raw backend state.

## Brand Personality

Calm, precise, operational.

The interface should feel like a dependable operations console: direct, legible, restrained, and confident under pressure. It should support expert workflows without becoming theatrical or hiding important detail behind friendly gloss.

## Anti-references

Avoid decorative SaaS dashboards: glossy cards, vanity metrics, busy gradients, and visual effects that make operations feel less trustworthy.

Avoid consumer-style simplification that hides operational detail, especially around permissions, budgets, request behavior, lifecycle state, and destructive actions.

## Design Principles

1. Operational truth over decoration. Show real system state, limits, maturity, and failure conditions plainly.
2. Safe by default. Administrative mutations should make scope, consequence, and reversibility clear before action.
3. Dense but legible. Preserve the detail operators need while keeping hierarchy, spacing, and scanning behavior disciplined.
4. Expose relationships. Make owners, teams, service accounts, model grants, budgets, and request activity traceable across surfaces.
5. Same-origin coherence. The UI should feel like the gateway's native control plane, not a separate marketing layer or generic admin template.

## Accessibility & Inclusion

Best-effort accessibility is the current baseline, with hardening expected during focused audits. Keyboard access, readable contrast, visible focus states, reduced-motion behavior, and responsive admin workflows should be treated as required quality gates for production-facing changes.
