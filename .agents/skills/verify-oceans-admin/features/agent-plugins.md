# Agent plugins

Agent Plugins is a placeholder page under Agent Extensions. It describes plugins that bundle MCP servers and skills, and shows an empty catalog until plugins ship.

## Sub-features

- `agent-plugins-empty` shows the `Plugin catalog` card with `No plugins yet` and `Agent plugins are coming soon.`

## How to get to it (user POV)

- Choose `Agent Plugins` under `Agent Extensions` in the sidebar.
- Open `/admin/agent-plugins` directly.

## Driving it with control-oceans-admin

Preconditions: a launched stack, signed in as `admin@local`.

- **Open.** Choose `Agent Plugins`. The URL is `/admin/agent-plugins` and the heading reads `Agent Plugins`.
- **Empty catalog.** The `Plugin catalog` card shows `No plugins yet`.
- **Proof.** Capture the page, then `control-oceans-admin record agent-plugins --check empty-catalog=pass --artifact <screenshot>`.

## Gotchas

- The page makes no API call yet, so there is no API parity check. Replace this recipe when the catalog is backed by data.
