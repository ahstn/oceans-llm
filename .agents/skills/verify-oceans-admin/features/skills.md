# Skills

Skills verification follows the real browser path from namespace selection and ZIP upload to version selection, download, and shared access. It compares displayed content with the production Skills API and uses native RustFS for archive storage.

## Sub-features

- `skills-entry` redirects anonymous users to sign-in and exposes Skills to a signed-in regular user.
- `skills-namespace` claims a permanent unique namespace and rejects changes or reuse by another user.
- `skills-upload` uploads a ZIP through the UI and confirms the owner, name, initial version, and digest through the API.
- `skills-preview` compares the displayed reference file with its saved API content.
- `skills-versions` appends a second immutable version, preserves the first default, and changes the default through the UI.
- `skills-download` downloads the selected version through the browser and confirms its SHA-256 digest; the first version stays unchanged.
- `skills-ownership` allows duplicate skill names in separate namespaces, shares reads, hides another user's owner controls, and rejects writes from both another user and a platform administrator.
- `skills-authentication` rejects anonymous reads and confirms service-account reads while denying all service-account write paths.
- `skills-cleanup` deactivates synthetic users and removes the run's database, object prefix, private configuration, and test token during stack teardown.

## How to get to it (user POV)

- Open `/admin/skills` and sign in, or follow the `Skills` sidebar link.
- Choose `Upload skill` to claim a namespace on first use and select a ZIP archive.
- Open the `namespace/skill-name` link to view instructions, versions, files, and download controls.
- Owners can use `Upload new version`, choose `Version`, and use `Set as default`.

## Driving it with control-oceans-admin

Preconditions:

- Set a fresh `OCEANS_VERIFY_RUN_ID`, free gateway/UI ports, and `OCEANS_VERIFY_SKILLS=true` before launch. Run all commands from the repository root.
- The `rustfs` mise profile must be installable and `lsof` must be available. Launch runs `mise -E rustfs run rustfs:setup`, which creates private local credentials, starts native RustFS, and ensures the Skills bucket exists.
- Use the default `gateway.yaml` or a `GATEWAY_CONFIG` with the `local-ci-runner` service account and its `primary` key. Launch creates a private copy with a synthetic service-account token, local object storage, and a separate `skills.db` beneath this run's state directory. It does not edit the source config or this checkout's `gateway.db`.
- Provider environment references must resolve for the gateway to start. For this Skills-only proof, absent `OPENAI_API_KEY`, `OPENAI_API_KEY_SECONDARY`, and `OPENROUTER_API_KEY` values can be synthetic placeholders. The driver sends no model request and does not prove provider authentication.
- Require `doctor` to pass. It checks the recorded gateway, UI, and RustFS listener PIDs and readiness. The existing checkout lock still prevents concurrent verification stacks.

```bash
export OCEANS_VERIFY_RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-skills-$$"
export OCEANS_VERIFY_GATEWAY_PORT=38090
export OCEANS_VERIFY_UI_PORT=33010
export OCEANS_VERIFY_SKILLS=true
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin launch
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin doctor
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive skills
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence skills
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin cleanup
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence skills
```

- **Enter and upload.** The driver opens the protected Skills route, captures the sign-in state, then creates two synthetic regular users through the production invitation API. It signs in through the browser, follows `Skills`, filters `Owner namespace`, and confirms `No skills found`. `Upload skill` opens `Choose your skill namespace`; the `Namespace` input and `Claim namespace` button persist the namespace. `Upload a skill` accepts the tracked `review-code-v1.zip` fixture through `ZIP archive`.
- **Read and version.** The detail heading must contain the owner's `namespace/review-code`. `skill-instructions` must contain version 1 instructions. The `Skill files` navigation opens `references/checklist.md`; `skill-file-text` must exactly match the production API. The driver uploads version 2, confirms that default version 1 remains selected in saved metadata, and uses `Set as default` to save version 2. `Download ZIP` must return bytes with the API's digest. Version 1 instructions and archive digest must remain unchanged.
- **Check ownership.** A second user fails to claim the first namespace, then uploads the same skill name in a different namespace. That user can view the first skill, while `Upload new version` and `Set as default` are absent. The production API must deny append and default changes from both this user and the platform administrator with HTTP 403.
- **Check caller types.** Anonymous catalog and archive requests must return HTTP 401. The synthetic service-account key receives HTTP 200 on reads and HTTP 403 for create, append, namespace claim, and default changes. Its value stays in the private run directory and process environment; proof files contain no credentials.
- **Retain proof and clean up.** Require `skills-proof.json` to contain `passed: true`. Screenshots and ARIA snapshots record entry, empty catalog, preview, both versions, the default change, another owner's view, and the final catalog. Driver cleanup deactivates its users. Stack cleanup removes only `skills-verify/<checkout-hash>/<run-id>/` from RustFS, confirms it is empty, removes the private database/config/token, and writes `skills-storage-cleanup.json`. It stops RustFS only when this run started the recorded process. `evidence skills` requires successful proof and, after teardown, successful storage cleanup.

## Gotchas

- The Skills launch uses a run-local database because Skills has no deletion API. Ordinary verification launch still uses this checkout's `gateway.db`.
- The RustFS bucket, local credentials, and data directory persist. Other object prefixes are outside this run's cleanup scope. A RustFS daemon that was already running stays running after cleanup.
- Always run cleanup after a failed launch or driver. A storage cleanup failure retains its recorded scope for a retry and returns a failure status.
- This proof uses real gateway, database, UI, and RustFS processes. It does not prove AWS S3 account permissions, Kubernetes/CSI deployment, Git import, or installed CLI behavior. The separate `mise -E rustfs run rustfs:e2e` task covers the CLI round trip.
- The service-account check uses the production configuration seed boundary. This proof does not create a provider request or change real user credentials.
- Preserve the run ID and evidence directory in the report. A health check or final screenshot alone is not proof of saved Skills behavior.
