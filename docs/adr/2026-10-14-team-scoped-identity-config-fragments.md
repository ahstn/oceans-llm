# ADR: Team-Scoped Identity Config Fragments

- Date: 2026-10-14
- Status: Proposed
- Builds On:
  - [2026-03-31-declarative-config-seeded-identity-and-budget-reconciliation.md](2026-03-31-declarative-config-seeded-identity-and-budget-reconciliation.md)
  - [2026-07-02-service-account-config-and-managed-api-key-secrets.md](2026-07-02-service-account-config-and-managed-api-key-secrets.md)
  - [2026-08-16-gateway-config-module-boundaries.md](2026-08-16-gateway-config-module-boundaries.md)

## Current state

- [../access/identity-and-access.md](../access/identity-and-access.md#declarative-identity-seed)
- [../configuration/configuration-reference.md](../configuration/configuration-reference.md#declarative-teams-and-users)
- [../operations/tagging.md](../operations/tagging.md)
- [../setup/runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)

## Context

Declarative identity config works well for a handful of teams. It does not scale to the deployments we now expect: 20 or more teams, 150 or more users, a default SSO provider, and per-user exceptions. Each team and each user can carry its own tags.

Today the whole identity graph lives in one YAML file:

- `GatewayConfig::from_path` in [../../crates/gateway/src/config/mod.rs](../../crates/gateway/src/config/mod.rs) reads exactly one file. It has no include, directory or overlay support. If the path does not exist, it silently returns `GatewayConfig::default()`.
- `teams`, `users` and `service_accounts` are lists. See [../../crates/gateway/src/config/identity.rs](../../crates/gateway/src/config/identity.rs).
- Every user repeats `auth_mode` and `oidc_provider_key`. There is no default provider for config-declared users. JIT defaults exist only per provider, for users the config does not list.
- Membership is declared on the user (`users[].membership`). A team's roster is spread across the user list, so a team's admins cannot own one place that describes their team.
- Tags are a list of `{key, value}` pairs.
- The Helm chart renders the whole config as a single ConfigMap key with `toYaml .Values.gateway.config`. See [../../deploy/helm/oceans-llm/templates/configmap.yaml](../../deploy/helm/oceans-llm/templates/configmap.yaml).

This produces a mega file with no natural ownership boundaries. It also interacts badly with Helm. Helm deep-merges maps across `-f` values files but replaces lists wholesale. So splitting `gateway.config.users` across several values files keeps only the last list.

A `conf.d` directory, as used by [mise](https://mise.jdx.dev/configuration.html), is the obvious shape. Admins will not have a predetermined set of files, though. Each deployment has its own X teams and Y users, so the chart cannot list file names. Merge semantics also need care. mise lets the last file win per key. That is safe for tool versions, but for access policy a silent override is a defect.

Two existing behaviours matter for any split:

- Reconciliation never prunes. Unlisted teams, users and service accounts are left alone. So deleting a file does nothing today.
- Seeding is not transactional. Every statement in [../../crates/gateway-store/src/postgres_store/seed.rs](../../crates/gateway-store/src/postgres_store/seed.rs) and the libSQL equivalent runs on the pool. A failure part-way leaves earlier upserts applied. More files mean more chances for that.

## Decision

### 1. Key identity collections by their stable identifier

`teams`, `users` and `service_accounts` become maps keyed by the identifier they already reconcile on. Teams are keyed by team key, users by email, and service accounts by service-account key.

```yaml
teams:
  research:
    name: Research
```

Maps compose by key, both inside the gateway loader and across Helm values files. A duplicate key inside one YAML document is a parse error. A duplicate key across documents is an explicit validation error (see decision 5). Users are keyed by email after normalization, so `Alice@Example.com` and `alice@example.com` collide.

The loader keeps accepting the current list forms through a custom deserializer and normalizes them to the map form. A file may not mix list and map forms for the same collection. List forms are deprecated and removed after one release that warns on them.

### 2. Teams own their roster and their service accounts

Membership moves from the user to the team:

```yaml
teams:
  research:
    name: Research
    tags:
      cost-center: "4410"
      org: rnd
    members:
      alice@example.com: { role: admin }
      bob@example.com: {}
    service_accounts:
      research-batch:
        budget: { cadence: monthly, amount_usd: "200" }
        keys:
          batch: { value: env.RESEARCH_BATCH_KEY, allowed_models: [gpt-5] }
```

- `members` maps email to `{ role }`. `role` defaults to `member`. `owner` is still rejected, as it is today.
- A user may appear in at most one roster. This matches the current one-team-per-user rule. Appearing in two rosters is a validation error that names both teams and their source files.
- `service_accounts` nest under their team. A service account cannot change team after creation, so nesting removes the redundant `team` field and puts automation budgets in the team's file.
- `users[].membership` and top-level `service_accounts` stay accepted in the legacy list form only. Declaring a membership both on a legacy user entry and in a roster is an error.

Membership semantics for config-declared users do not change. The set of config-declared users is the union of every roster email and every `users` key. A declared user's team is their roster entry, or no team if they are in no roster. This is the current "omitted `membership` removes the team" rule, stated against the roster instead of the user entry.

### 3. Users come from defaults, and `users` holds only exceptions

A new top-level `identity_defaults.user` block supplies the fields that every user currently repeats:

```yaml
identity_defaults:
  user:
    auth_mode: oidc
    oidc_provider_key: corp
    global_role: user
    request_logging_enabled: true
```

A roster email with no `users` entry is created from these defaults. Its display name defaults to the email local part, as JIT creation already does. A `users` entry overrides any default field and adds per-user budget and tags:

```yaml
users:
  carol@example.com:
    name: Carol Diaz
    auth_mode: password
    budget: { cadence: monthly, amount_usd: "500" }
    tags: { contractor: "true" }
```

- Validation applies the existing auth rules to the resolved user. For example, `auth_mode: oidc` still needs a provider key, and `password` must not carry one. Overriding `auth_mode` to `password` therefore clears an inherited provider key.
- If a roster email has no `users` entry and `identity_defaults.user` is absent, validation fails. This keeps today's explicitness when no default is configured.
- Budget defaults stay in `budgets.users`. `identity_defaults` does not get a second budget default.
- Provider JIT settings are unchanged. They still apply only to users that config does not declare.

### 4. Tags are a map, owned per entity

`tags` on teams, users and service accounts becomes a key-to-value map. The list-of-pairs form is accepted during the same deprecation window. A map cannot contain duplicate keys, which removes one validation error class.

Team tags and user tags remain separate entity tags. Config does not copy team tags onto users, and the store does not compute inherited tags. The existing rules are unchanged:

- at most `MAX_ENTITY_TAGS` (5) per entity;
- the key and value character rules;
- the reserved keys;
- omitted `tags` leaves stored tags unchanged, and an empty map clears them.

### 5. Load fragments from a config directory as a strict union

The gateway gains `--config-dir` (env `GATEWAY_CONFIG_DIR`). There is no default. Directory loading is opt-in, so an existing deployment never picks up stray files. These rules apply:

- The loader reads `*.yaml` and `*.yml` recursively, in lexical order of relative path.
- It skips any file or directory whose name starts with `.`. This is required for Kubernetes volume mounts, which contain `..data` and timestamped `..2026_...` directories.
- It follows symlinks, because ConfigMap mounts are symlinks.
- Each fragment is parsed as a `ConfigFragment` with `deny_unknown_fields`. A fragment may contain only `teams`, `users` and `service_accounts`. Everything else (`server`, `auth`, `models`, `identity_defaults`, `budgets`, `permissions`) stays in the main config file, where one owner controls it.
- The merge is a union by key. The same team, user or service account defined in two places is an error that names both source files, including when one of them is the main file. No file overrides another.
- Nested `members` and `service_accounts` belong to their team's entry. A team defined in one fragment cannot have members added from another fragment.
- `--config` and `--config-dir`, when set explicitly, must exist. A missing explicit path is a startup error, not an empty config. Only the implicit `./gateway.yaml` default may be absent.
- Validation runs once, on the merged config. Errors carry the source file of each entity involved.

A new `gateway config render [--show-sources]` command prints the merged config, optionally annotating each entity with its source file. `gateway config validate` accepts the same flags as `serve`.

### 6. Support two Helm mounting paths, and no chart-side file discovery

Both paths can be used in the same release.

**A. Values-file composition.** This needs nothing beyond decision 1. Identity maps stay under `gateway.config`, and admins compose them with Helm or Helmfile values layering:

```bash
helm upgrade oceans ./deploy/helm/oceans-llm -f values.yaml -f teams/research.yaml -f teams/infra.yaml
```

Helm deep-merges `gateway.config.teams.*` across the files. An overlay removes an entry with `research: null`. The chart still renders one `gateway.yaml`. This suits Helm-native users who keep one values file per team.

**B. Externally managed fragment ConfigMaps.** For GitOps repositories with a folder per team, the chart accepts a list of ConfigMap names:

```yaml
gateway:
  configDir:
    mountPath: /app/conf.d
    configMaps:
      - oceans-identity-research
      - oceans-identity-infra
```

The chart mounts them as one [projected volume](https://kubernetes.io/docs/concepts/storage/projected-volumes/), with each ConfigMap under its own subdirectory, and sets `GATEWAY_CONFIG_DIR`. Admins create the ConfigMaps however they like, for example with Kustomize `configMapGenerator`, `kubectl create configmap --from-file`, or another chart. The number and names of files are entirely theirs.

Chart changes required for either path:

- Mount the main config without `subPath`, or keep `subPath` for the main file and use a separate directory mount for fragments. `subPath` mounts never receive ConfigMap updates.
- Mount fragments into both the gateway pods and the seed-config job. The pods' init container runs `gateway config validate`, which should check the same merged config that the job seeds.
- Add `teams`, `users`, `service_accounts` and `identity_defaults` to `values.schema.json`.

Operational limits to document:

- ConfigMap keys allow only `[-._a-zA-Z0-9]`, so fragment files are named per team or per group, not per user email.
- A ConfigMap is capped at 1 MiB. Around 200 bytes per user puts a 150-user deployment well under that, even in one ConfigMap.
- The chart cannot checksum ConfigMaps it does not render. Pods do not seed (`GATEWAY_SEED_CONFIG=false`), so the seed job is what applies changes. Under Argo CD, Helm `post-install`/`post-upgrade` hooks run as `PostSync` hooks on every sync, so a fragment change triggers a reseed. Plain Helm users must run `helm upgrade` after changing an external fragment.

### 7. Ownership, pruning and transactional seeding come next, in that order

These are needed before team-owned files can be safely removed. They are a separate follow-up from the shape and loader changes.

1. **Transactional seed.** Wrap identity reconciliation (teams, service accounts and users, with their memberships, tags and budgets) in one transaction per backend. A failed seed then applies nothing. Provider and model seeding can stay outside the transaction if lock scope requires it.
2. **Ownership marker.** Add a `managed_by` column (`config` | `admin`) to teams, users and service accounts. Seeding sets `config` on rows it creates or reconciles. The admin UI shows config-managed fields read-only and explains where they come from. This stops admin edits from being silently reverted on the next seed.
3. **Opt-in pruning.** Add `seed.prune: none | deactivate`, defaulting to `none`. `deactivate` disables config-managed users, deactivates config-managed service accounts, and archives config-managed teams that are no longer declared. It never deletes rows and never touches `admin`-managed rows. Pruning refuses to run when the declared set is empty or a fragment failed to load. This stops a broken mount from deactivating everyone.

Pruning is the "explicit ownership mode" that the 2026-03-31 ADR listed as a follow-up.

## Implementation

Phased so each step is useful on its own:

1. **Shape** in [../../crates/gateway/src/config/identity.rs](../../crates/gateway/src/config/identity.rs):
   - map-or-list deserializers for the three collections and for tags;
   - nested `members` and `service_accounts` under teams;
   - `identity_defaults.user`;
   - resolution into the existing `SeedTeam`, `SeedUser` and `SeedServiceAccount` DTOs.

   The store contract does not change in this phase.
2. **Loader** in [../../crates/gateway/src/config/mod.rs](../../crates/gateway/src/config/mod.rs) and [../../crates/gateway/src/cli.rs](../../crates/gateway/src/cli.rs):
   - `--config-dir`;
   - the `ConfigFragment` type and union merge with source tracking;
   - the error for a missing explicit path;
   - `config render`.
3. **Chart** in [../../deploy/helm/oceans-llm/](../../deploy/helm/oceans-llm/):
   - `gateway.configDir`;
   - the projected volume on the gateway deployment and the seed job;
   - schema additions;
   - README examples for both mounting paths.
4. **Ownership** in [../../crates/gateway-store/src/seed.rs](../../crates/gateway-store/src/seed.rs) and both backend seed modules:
   - transaction wrapping;
   - the `managed_by` migration;
   - prune mode and its guards;
   - admin UI read-only state.

Docs to update with each phase:

- [../access/identity-and-access.md](../access/identity-and-access.md)
- [../configuration/configuration-reference.md](../configuration/configuration-reference.md)
- [../operations/tagging.md](../operations/tagging.md)
- [../setup/runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)

The configuration reference also has existing errors to fix in phase 1:

- the team field is documented as `key` instead of `id`;
- budgets are documented as "non-negative" when validation requires an amount greater than zero;
- `tags` is missing from the declarative identity section.

## Alternatives considered

- **Last-file-wins deep merge, as in mise `conf.d`.** Rejected for identity. A fragment could silently change another team's member role or a user's global role. A union with duplicate errors gives the same file layout without hidden overrides.
- **Chart-side discovery with `.Files.Glob` or `lookup`.** `.Files.Glob` reads only files packaged inside the chart, so chart consumers cannot use it. `lookup` returns nothing under `helm template` and in most GitOps renderers. Neither handles a dynamic, admin-owned set of files.
- **A label-watching sidecar, such as Grafana's `k8s-sidecar`.** This would collect ConfigMaps by label at runtime. It is unnecessary while seeding runs as a job, and it adds RBAC to list ConfigMaps cluster-wide or per namespace.
- **Keeping lists and adding per-file overlays.** This keeps every merge problem and only moves it.
- **A separate `identity.yaml` that is still a single file.** This does not create ownership boundaries.
- **IdP group-to-team mapping or SCIM.** Out of scope. Config remains the source of team membership and tags. Provider JIT stays as it is for users that config does not declare.

## Consequences

Positive:

- Each team can own one file that holds its name, tags, roster, service accounts and their budgets. CODEOWNERS can enforce that per file.
- A typical user needs no config entry beyond a roster line.
- Helm values layering and directory mounts both work without the chart knowing any file names.
- Cross-file mistakes fail validation with both file names, instead of applying silently.
- A missing explicit config path fails fast.

Trade-offs:

- Two accepted shapes for each collection during the deprecation window, with tests for both.
- Membership has one home, the team roster. A user's team is not visible from their `users` entry. `config render --show-sources` mitigates this.
- External fragment ConfigMaps are not covered by the chart's config checksum, so applying them depends on the seed job running.
- Until phase 4 lands, removing a fragment still leaves its teams and users in place, as it does today.

## Follow-up

- Decide whether `identity_defaults.user` should also accept a default OAuth provider, or only OIDC, once a deployment needs it.
- Revisit `MAX_ENTITY_TAGS` if team-owned tagging regularly needs more than five keys per entity.
- Consider whether `members` should accept a bare list of emails as shorthand for the `member` role.

## Attribution

This ADR was prepared through collaborative human + AI design work.
