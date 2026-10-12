# Review agent

Review Agent lets a platform administrator register a GitHub repository for automated pull-request reviews. The admin binds the repository to a service account, toggles review features, and generates the GitHub Actions workflow to commit. The page also lists recent review runs that the workflow reports back.

## Sub-features

- `review-repos-list` lists configured repositories with their service account, `N of 5 enabled`, last review, and status.
- `review-repo-create` adds a repository bound to a service account and opens its workflow setup.
- `review-repo-settings` edits the service account, the feature toggles, `Max inline comments`, and `Default model key`.
- `review-workflow` generates `.github/workflows/oceans-review-agent.yml` from `Action ref` and `API key secret name`.
- `review-repo-lifecycle` disables and reactivates a repository.
- `review-runs` lists recent runs and can filter them by repository.

## How to get to it (user POV)

- Under `Control Plane`, choose `Review Agent`. By default only platform admins are granted this page.
- Open `/admin/review-agent?repo_id=<id>&repo_section=overview|settings|setup` to land directly in `Manage repository`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as `admin@local`.
- The seeded service account `Local CI Runner` (Platform) is active.
- Use the repository name `verify-<run-id>` under the owner `oceans-verify`.

- **List.** Choose `Review Agent`. The `Review agent` heading shows `Repositories` and `Recent reviews`. With no repositories, the page shows `No repositories configured yet` and `Configure first repository`.
- **Create.** Choose `Add repository`, fill `Owner` and `Repository name`, pick `Local CI Runner (Platform)` in `Service account`, then choose `Configure repository`. The toast reads `Repository configured`, and `Manage repository` opens on `Workflow setup`.
- **Workflow.** `Generated workflow` shows YAML that names `.github/workflows/oceans-review-agent.yml` and the configured secret name. `Copy YAML` is present.
- **API parity.** Fetch `/api/v1/admin/review-agent/repositories`. The new repository appears with the same owner, name, and service account. Its row shows `3 of 5 enabled` and last review `Never`.
- **Settings.** In `Review settings`, set the `PR summary` toggle group to off, then choose `Save changes`. The toast reads `Repository updated`. Fetch the API again and check the row's feature count to confirm the change.
- **Runs.** Filter `Filter runs by repository` to the new repository. It shows `No review runs recorded yet.`, which matches `/api/v1/admin/review-agent/repositories/<id>/runs`.
- **Cleanup.** Under `Review settings` → `Lifecycle actions`, choose `Disable`. The toast reads `Repository disabled`, and both the row status and the API report it as disabled.
- **Proof.** Capture the create dialog, the workflow YAML (it contains no secrets), the updated toggle, and the disabled row.

## Gotchas

- This feature mutates `gateway.db`. There is no delete, so `Disable` is the cleanup and the record stays.
- Active repositories are unique by owner and name, case-insensitively. Always use a name that includes the run ID.
- Nothing calls GitHub. Runs appear only when a workflow posts to `/api/v1/review-agent/action/runs` with an API key, so an empty run list is expected.
- The `API key secret name` placeholder reads `OCEANS_API_KEY`, but the backend default is `OCEANS_REVIEW_AGENT_API_KEY`.
- Without an active service account, `No service accounts available` blocks creation.
- The backend accepts team owners and admins for their own team's accounts, but the default `gateway.yaml` grants the page only to platform admins.
