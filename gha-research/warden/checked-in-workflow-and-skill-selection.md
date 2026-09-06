# Checked-in workflow and skill selection

Source: [.github/workflows/warden.yml:1](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/.github/workflows/warden.yml#L1) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Exact workflow and warden.toml contents. The configuration selects Pi and two baseline skills; it does not activate the repository architecture-review skill.

```yaml
name: Warden

# contents: write required for resolving review threads via GraphQL
# See: https://github.com/orgs/community/discussions/44650
permissions:
  contents: write
  pull-requests: write
  checks: write

on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  review:
    runs-on: ubuntu-latest
    env:
      WARDEN_MODEL: ${{ secrets.WARDEN_MODEL }}
      WARDEN_OPENROUTER_API_KEY: ${{ secrets.WARDEN_OPENROUTER_API_KEY }}
      WARDEN_SENTRY_DSN: ${{ secrets.WARDEN_SENTRY_DSN }}
      WARDEN_SERVICE_URL: ${{ vars.WARDEN_SERVICE_URL }}
      WARDEN_SERVICE_TOKEN: ${{ secrets.WARDEN_SERVICE_TOKEN }}
    steps:
      - uses: actions/checkout@v4

      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '24'
          cache: 'pnpm'

      - run: pnpm install --frozen-lockfile
      - run: pnpm build
      - run: pnpm build:action

      - name: Analyze
        id: warden-analyze
        uses: ./
        with:
          mode: analyze

      - uses: actions/create-github-app-token@v1
        id: app-token
        with:
          app-id: ${{ secrets.WARDEN_APP_ID }}
          private-key: ${{ secrets.WARDEN_PRIVATE_KEY }}

      - name: Report
        uses: ./
        with:
          mode: report
          findings-file: ${{ steps.warden-analyze.outputs.findings-file }}
          github-token: ${{ steps.app-token.outputs.token }}
```

Repository configuration:

```toml
version = 1

[defaults]
runtime = "pi"
# Fail check on high+ severity findings (critical, high)
failOn = "high"
# Show annotations for medium+ severity findings
reportOn = "medium"
# Exclude build output and internal eval fixtures from all skills
ignorePaths = ["dist/**", "packages/evals/**"]

[[skills]]
name = "security-review"
paths = [
  "packages/warden/src/**/*.ts",
  ".github/workflows/*.yml",
  ".github/workflows/*.yaml",
  ".github/actions/**/*.yml",
  ".github/actions/**/*.yaml",
  "action.yml",
  "action.yaml",
]
ignorePaths = ["packages/warden/src/**/*.test.ts"]

[[skills.triggers]]
type = "pull_request"
actions = ["opened", "synchronize", "reopened"]

[[skills]]
name = "code-review"
paths = [
  "packages/warden/src/**/*.ts",
  ".github/workflows/*.yml",
  ".github/workflows/*.yaml",
  ".github/actions/**/*.yml",
  ".github/actions/**/*.yaml",
  "action.yml",
  "action.yaml",
]

[[skills.triggers]]
type = "pull_request"
actions = ["opened", "synchronize", "reopened"]
```
