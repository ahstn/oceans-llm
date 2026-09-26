# ADR: Profile Landing Page

- Date: 2026-09-26
- Status: Accepted

## Current state

- [Configuration Reference](../configuration/configuration-reference.md#permissions)
- [Identity and Access](../access/identity-and-access.md#admin-page-permission-groups)
- Amends [Configurable Admin Page Permissions](2026-08-05-configurable-admin-page-permissions.md)

## Context

After sign-in, the admin UI sent each user to their group's `default_page`. That page is usually a shared operational view, such as API Keys or Usage Costs. No single page showed a user their own budget, keys, and usage together.

A personal landing page must work for every group without new config. It also must not expose data outside the viewer's own scope.

## Decision

Paths in this ADR are admin router paths. The router is mounted under `/admin`, so `/profile` is served at `/admin/profile` and `/no-access` at `/admin/no-access`.

### 1. Signed-in users with page access land on `/profile`

`defaultSignedInPath` returns `/profile` when the session has at least one effective page. This applies to sign-in, `/`, and redirects away from pages the session cannot open. A session with no effective pages still goes to `/no-access`.

`/profile` is a personal path, like `/account/connections`. It is not a configurable page grant, so any active signed-in user can open it and config cannot hide it.

### 2. One self-scoped endpoint serves the page

`GET /api/v1/me/profile` returns only the session user's data:

- the active personal budget and its current-period spend
- a year of daily usage, with model and harness breakdowns
- the API keys the user owns personally

The handler reads the user from the session and takes no user identifier, so it has no cross-user access path. Personal keys come from the existing user-scoped API-key service query. The page does not call the admin API-key list, which returns global data for platform admins.

## Consequences

Benefits:

- every user starts on a view of their own budget, keys, and usage
- the page works for all permission groups without config changes
- sign-in does not load platform-wide key, user, or model lists

Trade-offs:

- `default_page` no longer controls where users land; the gateway still validates it and returns it in the session
- admins who start on an operational page now need one extra click
- profile aggregates run on each landing; they are bounded to one user and one year
- harness (client) breakdowns come from `request_logs`, like the admin harness reports. With `request_logging.purge` enabled, the Clients charts and Favourite client tile only cover the retention window, while token, model, and cost history come from durable usage accounting and cover the full year

## Follow-up work

- Decide whether to deprecate `default_page` or make the landing page configurable.
- Add a response cache if profile aggregates become slow for heavy users.
- Record the agent harness on durable usage accounting so client history survives request-log purges.

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
