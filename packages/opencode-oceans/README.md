# @ahstn/opencode-oceans

Discover Oceans and LiteLLM gateway models in OpenCode without copying model inventories into
`opencode.json`. This is a Bun-native TypeScript, Effect plugin for **OpenCode 2.2.6**. It uses
OpenCode's native provider adapters and catalog; it does not proxy inference requests.

## Install

Requires Bun **1.4.4 or newer**, OpenCode **2.2.6**, and its supplied Effect **4.1.0** runtime.
`@opencode/plugin` and `effect` are peers, not bundled dependencies. Node.js alone cannot load this
source-only package.

GitHub Packages requires authentication **even for public npm packages**. Create a GitHub personal
access token **(classic)** with `read:packages`, and provide it to the process running OpenCode:

```sh
export GITHUB_PACKAGES_TOKEN='your-token'
```

Add these lines to your user `~/.npmrc` (keep the variable reference, not the token value):

```ini
@ahstn:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_PACKAGES_TOKEN}
```

Configure `~/.config/opencode/opencode.jsonc`:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "plugins": ["@ahstn/opencode-oceans@0.1.0"],
}
```

OpenCode installs configured packages. You can alternatively run
`opencode plugin add @ahstn/opencode-oceans@0.1.0`. Use an exact version for reproducibility. The
registry token above downloads the plugin; it is **not** your gateway API key.

To use another gateway configuration directory, replace the plugin string with an object:

```jsonc
{
  "plugins": [
    {
      "package": "@ahstn/opencode-oceans@0.1.0",
      "options": { "configDir": "/absolute/path/to/opencode-config" },
    },
  ],
}
```

`configDir` is a directory, not a YAML filename. Use an absolute path; `~` is not expanded.

## Gateway configuration

By default the plugin reads `$XDG_CONFIG_HOME/opencode`, falling back to `~/.config/opencode`.
It prefers `gateways.local.yml` over `gateways.yml`. The local file **replaces**, rather than merges,
the shared file, even when empty or invalid. Missing files produce no gateway providers. Restart
OpenCode after editing gateway configuration.

The format is the OMP `providers` map. Only entries with `discovery.type: litellm` are used:

```yaml
providers:
  oceans:
    name: Oceans gateway
    baseUrl: https://gateway.example.com/v1
    apiKey: [OCEANS_API_KEY, LITELLM_API_KEY]
    api: openai-completions
    discovery:
      type: litellm
    modelOverrides:
      claude-sonnet-5-5:
        api: anthropic-messages
        input: [text, image, pdf]
        thinking:
          mode: anthropic-adaptive
          efforts: [low, medium, high]
          defaultLevel: medium
      gpt-6.1-sol:
        api: openai-responses
```

Replace the URL and model names with routes your gateway actually serves. `baseUrl` accepts a gateway
root or a URL ending in `/v1`; the plugin normalizes the suffix. HTTP(S) URLs must not contain embedded
credentials, queries, or fragments. HTTPS is recommended outside a local development environment.

`apiKey` is an environment variable name or ordered list of names, **never a raw key**, shell command,
or `os.environ/…` expression. OpenCode's active saved key takes precedence over environment keys.
Without a saved key, the first nonempty environment value wins. Connect the provider in OpenCode's Providers
UI to save a key. Without either credential, discovery is attempted without authentication. Gateway
IDs such as `oceans` identify both the provider and its saved integration credential.

Never commit raw gateway keys in YAML or `opencode.json`. The plugin registers environment and saved
key authentication methods instead of copying keys into model definitions. Discovery sends the key
only to the configured gateway and rejects redirects.

## Discovery and native adapters

Discovery tries `/model_group/info`, `/v2/model/info`, `/model/info`, and `/v1/model/info` for rich
metadata, stopping once the inventory has vision and input/output pricing information. It also reads
`/v1/models`; when available that authenticated listing determines visibility, including removals.
Missing endpoints (404/405) are skipped. Other HTTP errors, malformed responses, and timeouts retain
the previous successful inventory rather than replacing it with an incomplete result. Initial
failure leaves the provider empty and logs a warning. Embedding and image-generation inventories are
excluded.

Inventory refresh runs every 60 seconds inside the plugin's Effect scope. Each inventory is bound to the
connection that produced it, so switching accounts hides stale models until discovery succeeds. This does not poll YAML.
Malformed model entries fail the refresh and keep the last good inventory. A valid empty public list clears the inventory. On rich-only gateways, an empty result also clears it; an absent endpoint is not an empty result. Earlier rich endpoints keep their non-null metadata, including nested fields when a flat field is null.

Model IDs remain gateway IDs: for example, select `oceans/gpt-6.1-sol` in `/models`, or run:

```sh
opencode run --model oceans/gpt-6.1-sol "Explain this repository"
```

Each model selects an OpenCode-native package, not an AI SDK adapter:

| `api`                | OpenCode package                                     |
| -------------------- | ---------------------------------------------------- |
| `openai-completions` | `@opencode/ai/providers/openai-compatible`           |
| `openai-responses`   | `@opencode/ai/providers/openai-compatible/responses` |
| `anthropic-messages` | `@opencode/ai/providers/anthropic-compatible`        |

Rich route-pool facts outrank public-list hints: OpenAI-only pools use Responses; mixed pools use
Chat Completions. Authoritative client hints can select a native transport on other routes. A Claude
name alone does not prove that a gateway supports Anthropic Messages. An explicit per-model `api`
override wins; use one only when the gateway really exposes that protocol. The provider-level `api`
is a fallback, not a forced transport for every discovered model.

Gateway metadata and explicit overrides constrain input modalities and reasoning efforts. Compatible
first-party catalog entries supply missing metadata without borrowing metadata from another gateway.
Native Anthropic adaptive thinking uses `thinking.type: adaptive` and `effort`. Manual-thinking models
advertise `high` and `max` token budgets only when their Messages capabilities allow it; budgets stay below
the output limit and the 32,000-token ceiling. Images and PDFs are separate capabilities. Responses uses
`reasoningEffort` and `reasoningSummary: auto`. The plugin does not invent a full effort ladder when
the gateway explicitly limits it. `thinking.efforts` is an explicit user override; only advertise
levels the route supports. Select an advertised variant with a suffix such as
`oceans/gpt-6.1-sol#high`.

When native effort support is present but per-level flags are missing, the plugin keeps the gateway's hinted presets. Explicit per-level flags still set the allowed ladder. Missing input limits use the catalog's separate input limit, capped by the gateway context window.

## Usage and pricing limitations

OpenCode's native adapter reports actual response usage, including cache tokens when the gateway and
protocol expose them. The plugin does **not** estimate missing token counts or rewrite usage events.
It supplies catalog prices for OpenCode's local cost calculation: LiteLLM's per-token input, output,
and cache rates are converted to USD per million tokens. Explicit zero rates remain zero. Missing
cache rates fall back to a catalog cache rate, then to the input rate, rather than implying free
cached tokens. If no gateway rates exist, first-party catalog prices may be used.

Gateway cache rates override catalog cache rates even when input and output prices are absent.

An endpoint-reported charge overrides local catalog calculation, including a reported zero.
Without any known price, OpenCode displays zero; it has no unknown-price marker.
Displayed costs are **estimates, not billing records**. Gateway discounts, markups, routing,
long-context tiers, currency conversion, incomplete metadata, and missing upstream cache usage can
make them differ from invoices. An unknown price must not be interpreted as a free model. Rich
metadata endpoints may require permissions a gateway's normal inference key does not have; a
non-404/405 failure keeps the previous inventory instead of silently bypassing that error. No local
tokenizer, usage reconciliation, billing export, or background configuration-file watcher is provided.

## Develop

From the repository root:

```sh
mise run //opencode:install       # bun install --frozen-lockfile
mise run //opencode:typecheck     # TypeScript plus Effect diagnostics
mise run //opencode:diagnostics   # standalone Effect diagnostics
mise run //opencode:test
mise run //opencode:lint
mise run //opencode:fmt-check
mise run //opencode:fmt
mise run //opencode:smoke         # packed plugin in OpenCode 2.2.6; local fixture, no paid calls
mise run //opencode:check
mise run //opencode:pack          # checks, then source tarball in dist/
```

TypeScript 7.0.2 and `@effect/tsgo` 0.67.4 follow Effect's current TS7 recommendation. `tsconfig.json`
registers the native `@effect/language-service` plugin; the legacy JavaScript language-service package
is not needed with TS7. `effect-tsgo tsc` includes Effect diagnostics in CLI typechecking without
patching `node_modules`; Effect errors and warnings fail typechecking, suggestions do not. Configure
your editor's sole TypeScript LSP command, from this package directory, as:

```sh
bun run --bun effect-tsgo tsc@7.0.2 --lsp --stdio
```

Do not run the stock TS LSP alongside it. Lint extends the repository's `actions/.oxlintrc.json`;
formatter options match `actions/.oxfmtrc.json` (oxfmt has no `extends` configuration option).

## Release

The `OpenCode Oceans plugin` workflow checks pull requests and relevant `main` changes. Publishing is
manual only: update the package version and lockfile, create the matching existing tag
`opencode-oceans-v0.1.0`, and dispatch `.github/workflows/opencode-oceans.yml` **from main** with that
tag. The workflow validates the tag against the package version and checks the tagged commit before
publishing that exact commit with `bun publish --access public --tag latest` to GitHub Packages. Only
the publish job receives `packages: write`. No npmjs.org publication or bundled build is produced.

Non-release checks use the triggering commit SHA, so reruns do not check a newer branch revision.

GitHub initially creates npm packages as private, regardless of npm access flags: after the first
publication, a package administrator must set this package's visibility to **public** in GitHub
Packages settings. The repository link in `package.json` associates the package with this repository.
Public visibility still does not remove GitHub's installation-token requirement.

## References

- [OpenCode plugin configuration and installation](https://opencode.ai/docs/plugins/)
- [OpenCode Effect plugin lifecycle and provider transforms](https://opencode.ai/docs/build/plugins/effect/)
- [OpenCode TUI plugin boundary](https://opencode.ai/docs/build/plugins/cli/)
- [OpenCode Effect client](https://opencode.ai/docs/build/client/effect/)
- [OpenCode embedded Effect SDK](https://opencode.ai/docs/build/sdk/effect/)
- [OpenCode model selection, variants, and native provider options](https://opencode.ai/docs/models/)
- [LiteLLM model metadata and management endpoints](https://docs.litellm.ai/docs/proxy/model_management)
- [Effect language service: TS7 migration notice](https://github.com/Effect-TS/language-service)
- [Effect TS7 LSP, CLI diagnostics, and supported versions](https://github.com/Effect-TS/tsgo)
- [GitHub npm registry authentication and initial visibility](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry)
- [Bun source packaging and publishing](https://bun.sh/docs/pm/cli/publish)

Licensed under AGPL-3.0-only; see [LICENSE](./LICENSE).
