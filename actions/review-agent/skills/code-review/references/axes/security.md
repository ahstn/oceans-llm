# Security & Trust Boundaries Axis

Focus on untrusted data, identity, permissions, and exposure.

Look for:

- missing validation or unsafe parsing at trust boundaries
- missing authn/authz or tenant checks
- injection paths into SQL, shell, templates, files, or URLs
- secret exposure in code, logs, telemetry, or errors
- unsafe deserialization, path traversal, SSRF, or XSS-style flows where relevant

Only report issues with a concrete exploit or leakage path.

## Source, boundary, sink, and mitigation

For each candidate, show attacker control, the protected boundary, the vulnerable operation or missing guard, and concrete impact. Follow middleware, validators, ownership checks, parameterization, escaping, allowlists, signatures, and filesystem containment through the effective path. A dangerous API or missing nearby check is only a lead.

For CI, trace PR-controlled code, expressions, artifacts, caches, and dependencies into tokens, secrets, privileged runners, releases, or deployments. Verify which revision supplies workflow code and local actions. A broad permission or mutable ref alone is not enough; show the path to a privileged effect.

Check actual framework behavior before alleging injection, XSS, SSRF, or unsafe deserialization. Distinguish real exposed credentials from test placeholders. Generated code, scripts, and migrations still require review when executed with sensitive access. Do not propose guards for values that a verified boundary already excludes.
