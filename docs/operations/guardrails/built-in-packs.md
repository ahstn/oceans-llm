# Built-in Deterministic Packs

Built-in packs evaluate destructive commands and structured tool operations inside the gateway. They do not call an external policy service. Use them when an operation can be identified from its executable, command structure, tool identity, or typed arguments.

`See also`: [Gateway Guardrails](../gateway-guardrails.md), [Agent Harness Usage](../agent-harness-usage.md), [MCP Tool Access](../../mcp/mcp-tool-access.md)

## How deterministic matching works

Shell checks parse command structure rather than applying a regular expression to raw text. The parser follows nested shell commands, command chains, pipelines, options, and redirections so quoted data is not treated as an executable operation.

Generated tool checks recognize common shell-tool identities and inspect command fields such as `command`, `cmd`, `input.command`, and `arguments.command`. MCP checks use the server identity, canonical tool identity and aliases, parsed JSON arguments, and typed JSON-path predicates.

A matching pack produces a stable pack ID, rule ID, reason code, matched field, description, and safer action. In `audit` mode the operation continues. In `deny` mode the gateway stops it at the protected boundary.

## Available packs

| Pack | Coverage |
| --- | --- |
| `core.shell` | Host power-state changes, filesystem formatting, termination of init or broad process groups, dynamic executables, and shell input that cannot be inspected |
| `core.git` | Commands that discard working-tree state, rewrite or delete refs, delete branches or worktrees, remove stashes, and expire recovery data |
| `core.filesystem` | Recursive deletion, path removal, file shredding or truncation, destructive redirection, device overwrite, signature erasure, disk reformatting, and `find -delete` |
| `database.postgresql` | Destructive PostgreSQL CLI and SQL operations, including SQL supplied through a pipeline that the command guard cannot inspect |
| `database.snowflake` | Destructive Snowflake CLI and SQL operations, object and staged-file removal, destructive application or service operations, and SQL input that cannot be inspected |
| `cloud.aws` | Destructive AWS CLI and structured MCP operations across supported AWS services |
| `cloud.gcp` | Destructive Google Cloud CLI and structured MCP operations across supported Google Cloud services |
| `kubernetes.kubectl` | Resource deletion, namespace-wide or forced deletion, node disruption, scale-to-zero, destructive apply or replace, storage deletion, and RBAC removal |
| `kubernetes.helm` | Release uninstall, rollback, forced replacement, value reset, and cleanup behavior that can delete resources |
| `secrets.aws_secrets` | Mutation or deletion of AWS Secrets Manager secrets, versions, policies, rotation or replication state, and SSM parameters |
| `secrets.onepassword` | Deletion or replacement of 1Password items, documents, vaults, users, groups, tokens, access grants, and Connect servers |
| `secret_disclosure` | Commands that print stored credentials, generated credentials, tokens, protected documents, or decrypted values to model-visible output |
| `saas.github` | Destructive GitHub MCP repository, file, workflow, pull request, review, project, issue, label, relationship, and discussion-comment operations |
| `saas.notion` | Destructive Notion MCP page, block, database, property, comment, and workspace operations covered by the pack |

Pack IDs are versioned policy contracts. Startup rejects an unknown ID rather than silently skipping it.

## Enable mutation-focused packs

The checked-in development and production configurations enable the mutation-focused catalog in audit mode:

```yaml
guardrails:
  default:
    enabled: true
    mode: audit
    packs:
      - core.shell
      - core.git
      - core.filesystem
      - database.postgresql
      - database.snowflake
      - secrets.aws_secrets
      - secrets.onepassword
      - cloud.aws
      - cloud.gcp
      - kubernetes.kubectl
      - kubernetes.helm
      - saas.github
      - saas.notion
    managed_checks: []
```

Keep the initial policy in `audit` while representative traffic exercises each enabled pack. Review the pack, rule ID, matched field, and safer action for every unexpected match before enabling `deny`.

## Protect secret values

`secret_disclosure` is an explicit opt-in pack. Unlike the provider-specific secret packs, which detect destructive mutations, it detects commands that expose credential values in output visible to the model or caller.

Coverage includes value-returning operations from:

- AWS Secrets Manager and decrypted SSM parameters
- 1Password CLI
- HashiCorp Vault
- Infisical
- Doppler

Enable it first on a narrow model route or MCP server:

```yaml
guardrails:
  default:
    enabled: true
    mode: audit
    packs: [core.shell, core.git, core.filesystem]
    managed_checks: []
  model_routes:
    agent/openai-prod/gpt-5:
      mode: audit
      packs:
        - core.shell
        - core.git
        - core.filesystem
        - secret_disclosure
```

The pack allows help output and recommends injection-oriented workflows such as `op run`, `doppler run`, or `infisical run` where applicable. It does not make secret output safe to retain elsewhere. Keep request-payload capture and incident handling aligned with the existing sensitive-data policy.

## Understand important pack behavior

### Git

`core.git` distinguishes state-preserving commands from operations that discard or rewrite data. Examples include hard reset, forced clean, force push, remote-ref deletion, branch deletion, forced worktree or submodule changes, stash removal, and immediate pruning of recovery history.

Normal branch switching, dry-run clean, and non-destructive pushes do not match merely because a related destructive operation exists.

### PostgreSQL and Snowflake

The database packs inspect SQL provided directly on the command line. They also deny or audit input from files, stdin, pipelines, or unresolved shell expansion when the exact SQL cannot be inspected safely.

This is deliberately conservative. Materialize and review the exact SQL before execution rather than moving destructive statements behind an opaque input source.

### Kubernetes and Helm

`kubernetes.kubectl` recognizes dry-run behavior. Preview operations using `--dry-run=client` or `--dry-run=server` do not match the corresponding live mutation rule.

`kubernetes.helm` similarly permits effective dry runs while covering uninstall, rollback, forced upgrade, reset values, and cleanup-on-failure options. A preview is evidence for review, not approval to perform the live operation.

### GitHub MCP

`saas.github` requires a GitHub server identity and an exact supported tool or method shape. It does not classify arbitrary JSON by searching serialized text.

The pack covers repository and file deletion, replacement of an existing file, workflow cancellation and log deletion, pull request merge or closure, pending-review deletion, project state deletion, issue closure or field deletion, relationship removal, label deletion, and discussion-comment deletion or replacement.

## Roll out deny mode

1. Enable the required packs in `audit` mode.
2. Exercise normal and intentionally destructive examples for each protected workflow.
3. Review decision records in **Observability > Guardrails**.
4. Confirm near misses remain allowed and matches identify the expected field and rule.
5. Move one model route or MCP server to `deny`.
6. Verify that a denied operation does not cross the provider, MCP, or local process boundary.
7. Expand enforcement after the observation window.

To stop enforcement without losing decision history, return the affected scope to `audit`. If pack evaluation causes unacceptable compatibility problems, set `enabled: false` only on that scope while investigating.
