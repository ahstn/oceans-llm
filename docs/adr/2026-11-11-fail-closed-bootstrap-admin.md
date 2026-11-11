# Fail closed for bootstrap administrator startup

## Decision

Gateway startup fails when its selected configuration file is missing. Built-in defaults bind to loopback, leave the bootstrap administrator disabled, and do not contain a password. `gateway serve` does not run bootstrap unless an operator explicitly enables it; `gateway bootstrap-admin` remains the one-shot alternative.

An enabled bootstrap administrator on a non-loopback listener must use an `env.*` or `file.*` password reference. The resolved password must contain at least 16 characters and 8 distinct characters, and first-login password rotation must be enabled.

## Implementation

- `GatewayConfig::from_path` reads the requested path unconditionally, so a missing file is an error for every runtime and maintenance command.
- `ServerConfig`, `BootstrapAdminConfig`, and `ServeArgs` use fail-closed defaults.
- Configuration validation enforces the external-secret, password-quality, and rotation requirements before any listener or database bootstrap work begins.
- Production-shaped mise, Compose, and self-host entry points explicitly opt in to bootstrap and provide the password through an environment reference.
- The checked-in local demo remains an explicit exception: it binds only to loopback and may use its documented development credential.

## Rationale

The previous fallback combined a wildcard listener, automatic bootstrap, and a repository-known administrator password. A missing path or incomplete configuration could therefore turn an operator mistake into a remotely reachable privileged session. Enforcing the invariant during configuration loading closes every command path before the administrator row or listener can be created.

## Trade-offs

- Existing deployments that relied on a missing file, implicit bootstrap, or a literal password on a non-loopback listener now fail at startup and must provide explicit configuration.
- A minimum-length and character-diversity check is intentionally simple. It blocks known placeholders and low-variety values without adding a password-strength dependency, but operators remain responsible for generating a unique secret.
- Loopback development configurations retain literal-secret support so offline demos and deterministic tests remain usable. They cannot be exposed safely without changing to an external secret and forced rotation.

## Follow-up

Consider replacing reusable bootstrap passwords with a single-use, expiring enrollment token if the product adds a dedicated first-run workflow.
