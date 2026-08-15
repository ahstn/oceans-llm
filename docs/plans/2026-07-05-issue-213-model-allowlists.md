# Issue 213: Model-level allowlists for users and teams

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Data Relationships](../contributing/reference/data-relationships.md), [Budgets](../access/budgets.md)

## Goal

Add an optional model-level `allowlist` to model configuration so a gateway model can be restricted to specific human users and/or teams from YAML.

This is model-centric policy: it answers "which principals may use this model?" It must remain separate from existing principal-centric restrictions that answer "which models may this principal use?"

## Non-negotiable invariants

1. Models without `allowlist` preserve existing behavior.
2. Do not seed or reuse `user_model_allowlist`, `team_model_allowlist`, or `service_account_model_allowlist` for this policy.
3. Do not mutate `users.model_access_mode`, `teams.model_access_mode`, or `service_accounts.model_access_mode`.
4. Store model-level allowlist refs as strings, not user/team foreign keys.
5. Unknown users and teams are valid future-effective refs.
6. `allowlist` omitted means no model-level deny policy and clears prior policy during config seed reconciliation for that configured model.
7. `allowlist: {}` or both arrays empty is invalid config.
8. Human user-owned keys pass a model allowlist when the normalized user email OR effective team key is listed.
9. Service-account-owned keys are denied for allowlisted models in v1.
10. Aliases are independent gateway model keys; allowlists do not inherit between alias and target.
11. `tag:` selectors evaluate effective accessible models and skip blocked candidates.
12. Admin API and UI expose the policy read-only in v1; no editing surface.

## Implementation phases

### 1. Config and core domain

Files:

- `crates/gateway/src/config.rs`
- `crates/gateway-core/src/domain.rs`
- `crates/gateway-core/src/traits.rs`

Add `ModelAllowlistConfig` to YAML config:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ModelAllowlistConfig {
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub teams: Vec<String>,
}
```

Add `allowlist: Option<ModelAllowlistConfig>` to `ModelConfig`. Use `Option` so omitted and explicit empty remain distinguishable.

Add core policy DTO:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelAllowlistPolicy {
    pub users: Vec<String>,
    pub teams: Vec<String>,
}
```

Add `allowlist: Option<ModelAllowlistPolicy>` to `SeedModel`.

Config validation:

- normalize users with existing `normalize_config_email`
- normalize teams with existing `normalize_config_team_key`
- dedupe deterministically
- reject present allowlists where both normalized arrays are empty
- do not check refs against configured or stored users/teams

Config tests:

- parse and normalize user/team refs
- unknown refs accepted
- omitted allowlist maps to `None`
- `allowlist: {}` rejected with model name
- both arrays empty rejected with model name

### 2. Persistence and seed reconciliation

Files:

- `crates/gateway-store/migrations/V37__model_allowlists.sql`
- `crates/gateway-store/migrations/postgres/V37__model_allowlists.sql`
- `crates/gateway-store/src/migration_registry.rs`
- `crates/gateway-store/src/migrate.rs`
- `crates/gateway-store/src/libsql_store/models.rs`
- `crates/gateway-store/src/postgres_store/models.rs`
- `crates/gateway-store/src/libsql_store/seed.rs`
- `crates/gateway-store/src/postgres_store/seed.rs`
- `crates/gateway-store/src/lib.rs`

Add dedicated tables:

```sql
CREATE TABLE IF NOT EXISTS model_allowlist_users (
  model_id TEXT NOT NULL,
  normalized_email TEXT NOT NULL,
  PRIMARY KEY (model_id, normalized_email),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS model_allowlist_users_email_idx
  ON model_allowlist_users (normalized_email, model_id);

CREATE TABLE IF NOT EXISTS model_allowlist_teams (
  model_id TEXT NOT NULL,
  team_key TEXT NOT NULL,
  PRIMARY KEY (model_id, team_key),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS model_allowlist_teams_team_key_idx
  ON model_allowlist_teams (team_key, model_id);
```

No FKs to users or teams.

Add V37 to `migration_registry.rs` and add both table names to app-table detection in `migrate.rs`.

Add `ModelRepository` allowlist reads. Correction: make the batch method required and only provide a one-way default for direct lookup. Do not define two defaults that call each other.

```rust
async fn list_model_allowlists_for_models(
    &self,
    model_ids: &[Uuid],
) -> Result<HashMap<Uuid, ModelAllowlistPolicy>, StoreError>;

async fn get_model_allowlist(
    &self,
    model_id: Uuid,
) -> Result<Option<ModelAllowlistPolicy>, StoreError> {
    let mut policies = self.list_model_allowlists_for_models(&[model_id]).await?;
    Ok(policies.remove(&model_id))
}
```

Seed reconciliation for each configured model:

1. Delete existing rows in both model allowlist tables for that model id.
2. If `allowlist` is `Some`, insert normalized users and teams exactly.
3. If `allowlist` is `None`, insert nothing; this clears prior stored policy.
4. Do not touch unconfigured models.

Store tests:

- present policy persists unknown refs
- second seed replaces policy exactly
- omitted policy clears stored rows
- alias and target policies are independent
- empty batch read returns empty map
- libsql and Postgres behavior match

### 3. Runtime enforcement

Files:

- `crates/gateway-service/src/model_access.rs`
- `crates/gateway-service/src/service.rs` only for trait-bound fallout

`ModelAccess` is the single runtime owner. Do not duplicate policy in HTTP handlers, model resolver, route planner, or budget code.

Effective access order:

1. Validate existing API-key grant shape.
2. Apply API-key grants.
3. Apply existing principal-centric restrictions.
4. Apply model-level allowlist policy.

Model-level rule:

- no policy: allow unchanged
- policy exists + service-account-owned key: deny in v1
- policy exists + human user-owned key: allow when user email OR effective team key matches

Preserve direct request optimization: direct all-mode resolution should not load the whole catalog; it should load only the requested model policy.

Denied direct requests should use existing not-granted/unauthorized behavior.

`tag:` behavior should fall out of `effective_models_for_api_key` filtering; blocked tagged candidates are skipped.

Budget behavior stays unchanged. Allowlist denial happens before budget checks; allowed requests use existing execution-model/upstream-model budget scopes.

Runtime tests:

- listed user access
- listed team member access
- unlisted user with API-key grant blocked
- service-account key blocked for allowlisted model
- model without allowlist unchanged
- alias and target policy independence
- `tag:` skips blocked higher-ranked model
- all blocked tagged candidates use existing unavailable behavior
- direct all-mode resolution still avoids catalog load

### 4. Admin API and UI

Files:

- `crates/gateway-service/src/admin_models.rs`
- `crates/gateway/src/http/admin_contract.rs`
- `crates/gateway/src/http/models.rs`
- `crates/gateway/openapi/admin-api.json` generated
- `crates/admin-ui/web/src/generated/admin-api.ts` generated
- `crates/admin-ui/web/src/routes/models.tsx`
- `crates/admin-ui/web/src/server/admin-data.server.test.ts`
- `crates/admin-ui/web/src/server/admin-preview-data.server.ts`
- `crates/admin-ui/web/src/test/routes/models-route.test.tsx`

API shape:

```rust
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminModelAllowlistView {
    pub users: Vec<String>,
    pub teams: Vec<String>,
}

pub struct AdminModelView {
    // existing fields
    pub allowlist: Option<AdminModelAllowlistView>,
}
```

`None` means unrestricted by model-level policy.

Admin service should batch-load policies and map policy for the displayed model row, not the resolved execution model.

UI should display policy read-only on Models:

- null: `Unrestricted by model allowlist`
- non-null: show users and teams as normalized refs
- no edit forms, save buttons, or links that imply editing in v1

Regenerate and verify contract artifacts:

```bash
mise run admin-contract-generate
mise run admin-contract-check
```

### 5. Documentation

User-facing docs:

- `docs/configuration/configuration-reference.md`: YAML shape, normalization, unknown refs, omitted-vs-empty, seed semantics, alias behavior, `tag:` behavior, service-account v1 denial.
- `docs/access/identity-and-access.md`: distinguish API-key grants, principal-centric restrictions, and model-centric allowlists.
- `docs/configuration/model-routing-and-api-behavior.md`: `tag:` effective model filtering and alias independence.
- `docs/reference/request-lifecycle-and-failure-modes.md`: allowlist gate happens before route planning and budgets.
- `docs/access/service-accounts.md`: service-account v1 caveat for allowlisted models.
- `docs/access/admin-control-plane.md`: Models page displays allowlist read-only.
- `docs/access/budgets.md`: user-facing budget taxonomy and setup guidance; explain budgets are spend controls, not model authorization.

Developer/contributor docs:

- `docs/contributing/reference/data-relationships.md`: new tables, string refs, no FKs to users/teams, authorization semantics.
- `docs/contributing/operations/budgets-and-spending.md`: only note allowlist runs before budget guard if needed.
- Optional ADR under `docs/adr/` if the dedicated storage/no-FK/service-account-v1 choices need permanent decision record.

### 6. Verification

Targeted during implementation:

```bash
cargo test -p gateway config
cargo test -p gateway-store model_allowlist
cargo test -p gateway-service model_access
cargo test -p gateway-service admin_models
bun run --cwd crates/admin-ui/web test
mise run admin-contract-check
mise run //docs:build
```

Final required verifier for this mixed Rust/UI/docs change:

```bash
mise run lint
```

## Risks

- Erasing omitted-vs-empty by using plain vectors instead of `Option`.
- Reusing principal-centric tables and changing unrelated model access.
- Checking alias policy after canonicalization and accidentally inheriting policy.
- N+1 policy loads on `/v1/models`, admin model list, or `tag:` selectors.
- Partial seed failure after delete-before-insert. Prefer transaction coverage if it can be done cleanly across both stores.
- Blurring access policy and budget policy in docs.
