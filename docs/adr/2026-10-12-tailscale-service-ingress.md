# ADR: Tailscale Service for Self-Hosted Ingress

- Date: 2026-10-12
- Status: Accepted
- Amends: [Self-hosting with mise daemons](2026-10-01-self-hosting-with-mise-daemons.md), decision 6

## Context

Decision 6 of the self-hosting ADR published the gateway with node-level Tailscale Serve: `https://hogwarts.<tailnet>.ts.net` on port 443. That ties the URL, OAuth callbacks, and API clients to one host name. It also takes the host's port 443, which other services on the same host then cannot use.

## Decision

Publish the gateway as the Tailscale Service `svc:oceans-llm`, at `https://oceans-llm.<tailnet>.ts.net`. `selfhost-expose` runs `tailscale serve --service=svc:oceans-llm --https=443 http://127.0.0.1:8080`, and removes the earlier node handler while that handler still points at the gateway.

The tailnet side lives outside this repository: the Service definition, the host tag, the auto-approver, and the access grant. For `hogwarts` they are managed with OpenTofu in `ahstn/dotfiles` under `tailnet/`.

## Consequences

- The URL stays the same if the gateway moves to another host, or if a second host advertises the Service.
- The Service has its own virtual IP, so the host's own port 443 is free again.
- Access is granted to `svc:oceans-llm`, not to the whole host.
- The host must use a tag-based identity. A tagged host leaves `autogroup:self`, so Tailscale SSH needs a rule for the tag.
- Clients need Tailscale 1.94 or later to reach Services without `--accept-routes`.
- The first deploy after this change needs `GATEWAY_PUBLIC_BASE_URL` set to the Service name.
