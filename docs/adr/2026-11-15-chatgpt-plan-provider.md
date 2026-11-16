# ChatGPT plan provider through Sign in with ChatGPT

## Status

Accepted. The upstream Sign in with ChatGPT (SIWC) flow is a preview; this provider is restricted to personal, single-owner deployments until Oceans has per-user ChatGPT credentials.

## Context

Oceans needs a provider for inference authorized against a person's ChatGPT plan, rather than an OpenAI API key. The existing `openai_compat` adapter does not own OAuth registration, renewable local credentials, or SIWC's Responses restrictions.

The official SIWC open-source contract uses `POST https://api.openai.com/v1/responses` with the OAuth access token as a bearer credential. This is **not** the legacy `https://chatgpt.com/backend-api/codex` transport used by omp's `openai-codex` provider. Reusing that transport, its token files, or its account/agent headers would conflate two distinct authentication and inference contracts.

SIWC requires streaming HTTP Responses requests with `store: false`, full context in an `input` array, and no persistent server-side continuation. Its account-specific model catalog also differs from the ordinary OpenAI API catalog: entries are `models[].slug`, not `data[].id`.

## Decision

### Dedicated provider, public Responses transport

Add `chatgpt_plan` as a first-class provider with a dedicated OAuth token source and request-normalization boundary. Reuse the existing Responses streaming machinery for forwarding, usage capture, cancellation, and request accounting rather than introducing an SDK or child process.

- Default `base_url` is `https://api.openai.com/v1`; inference appends `/responses`.
- Authenticate with `Authorization: Bearer <access_token>`. Do not synthesize `ChatGPT-Account-Id`, `originator`, or `OpenAI-Beta` headers from the legacy Codex transport.
- Serve streaming Responses only. Do not translate Chat Completions or Anthropic Messages, and do not expose embeddings, image generation, Decisions, batches, or response storage/retrieval through this provider.
- Use explicit, static model routes. No upstream model discovery runs as part of this implementation. A gateway model such as `chatgpt/gpt-6-luna` maps explicitly to upstream `gpt-6-luna`; its name alone does not prove account eligibility.
- Require `stream: true`; do not silently turn a non-streaming request into an SSE response. Forward both `response.completed` and `response.incomplete` as terminal events, including their usage; an incomplete response is not completed inference. Treat `response.failed`, explicit errors, and streams without a terminal event as errors.

### Required normalization versus optional stripping

`strip_unsupported_fields` defaults to `true`. It controls only removal of optional request fields that the SIWC preview does not accept. Setting it to `false` is useful when diagnosing changing upstream capabilities; it is not an unrestricted OpenAI API mode.

Required protocol rewrites always apply after caller and route body settings: force `store: false`, retain the streaming requirement, normalize input to an array, and adapt message roles to the SIWC contract. Caller input or route `extra_body` must not defeat these requirements. HTTP continuation must carry history in `input`, not rely on `previous_response_id`. Unsupported endpoints and non-streaming requests remain unsupported regardless of the stripping toggle.

Do not claim every Responses capability is available. SIWC's preview limits exclude hosted image generation, file search, Code Interpreter, native computer use, hosted MCP/connectors, `tool_search`, and top-level `programmatic_tool_calling`. Local tool execution is distinct from hosted Responses tools. Follow the selected model's input and tool restrictions; audio/video, the Files upload API, and transcription are not part of this flow.

### Local login and renewable file ownership

`oceans providers login-chatgpt` performs the official loopback authorization-code flow with PKCE, state and nonce validation, dynamic registration for a new account, and validation of the returned ID token. It saves the issued client ID, verified identity, stable host ID, tokens, granted scopes, and expiry metadata. An ID token alone is not permission to infer: the granted scopes must include `chatgpt.tokens.use.direct`.

The provider's `credentials_file` is a plain, mutable filesystem path, not an `env.*`, `literal.*`, or other value-source expression. A leading `~` is supported. The gateway needs write access to the file and its parent directory because refresh atomically replaces the credential record. Use protected storage with owner-only permissions; never put tokens in YAML, source control, logs, URLs, or support transcripts.

The token source owns refresh and persistence together. Serialize concurrent refreshes within the process and keep refresh cancellation-safe: a disconnected inference caller must not discard a replacement refresh token after OpenAI has rotated the old token. Publish refreshed credentials only as a complete, atomically persisted token set. The source caches the parsed credential record and checks file modification time and length off the async executor; it reloads atomic login replacements without a gateway restart and re-reads before near-expiry refresh. Keep this cache inside the persistence owner, without a second access-token cache that could split token lifetime from persistence ownership.

**Exactly one gateway process owns a credential file/session.** Refresh tokens rotate, so an in-process lock cannot make sharing one session across gateway replicas safe. Perform a separate login for each gateway process and give it an independent credential file. Do not copy one renewable session to another host or point two providers at the same file. Pause inference during reauthorization so the CLI's atomic credential replacement cannot race an active refresh; the running gateway detects the replacement without a restart.

Preserve credentials on temporary network, admission, and availability failures. Terminal refresh-token failures require reauthorization with the saved issued client registration; invalid client configuration requires a configuration fix, not repeated OAuth. Do not silently switch to API-key billing when plan admission or usage limits fail.

### Single-owner security boundary

This first version has **one ChatGPT account per provider**, not one account per gateway user. Every gateway API key allowed to use a model routed to that provider can consume that account's plan, regardless of the key's user or service-account owner. Gateway authentication, attribution, budgets, and model grants do not create separate upstream identities or entitlements.

OpenAI's terms prohibit making the owner's account available to others. This is not an organization seat pool or a way to share one subscription across gateway users. Restrict model grants to the account owner's own keys; avoid wildcard/`all` grants that unintentionally include current or future ChatGPT routes. Model allowlists can further narrow access but do not replace per-user upstream credentials.

**Do not enable `chatgpt_plan` in a multi-user deployment until per-user credentials are implemented**, following the isolation model of Copilot's `github_user`: select the exact caller's own stored provider credential, fail closed if absent, and never fall back to a provider-wide account.

### Accounting is not subscription billing

Use `pricing_provider_id: openai` to attribute usage at API-equivalent catalog rates for the explicitly configured upstream model. These amounts support gateway attribution and budgets; they are not the price of the ChatGPT subscription, a measurement of remaining plan quota, or an OpenAI bill. A catalog match does not prove model entitlement, and a model without a catalog match needs an explicit pricing decision rather than an invented price.

The account owner reviews actual app limits, credits, and access in [ChatGPT Settings → Usage](https://chatgpt.com/settings/usage). Plan limits may be shared across apps. A usage-limit error does not by itself establish a reset time or mean the entire plan is exhausted.

## Consequences

- A dedicated adapter keeps preview-specific normalization and renewable credentials out of generic OpenAI API-key providers.
- Existing gateway model access, request logs, streaming usage capture, and attribution continue to apply, but must not be mistaken for upstream per-user isolation.
- Static routes and streaming-only support keep the initial supported surface explicit. API-shaped similarity is not permission to use another endpoint.
- Single-process credential ownership intentionally excludes replica sharing and shared subscription deployments.

## Follow-ups

1. Add encrypted per-user ChatGPT registrations, account selection, and trusted API-key-owner credential resolution before supporting multi-user deployments. Follow Copilot `github_user` isolation rather than a shared fallback account.
2. Add namespace adaptation for function/custom tools if the SIWC endpoint rejects flat tools. Preserve tool names, call IDs, history, and tool-choice semantics end to end; do not advertise unsupported hosted tools as a workaround.
3. Add non-streaming Responses aggregation only with explicit handling of terminal success, incomplete responses, usage, failures, interruption, and cancellation. Until then, reject non-streaming calls.
4. Consider account-scoped SIWC model discovery. Parse `models[].slug` and `display_name`, respect `visibility: "list"` and server ordering, and refresh when the selected account changes. Do not reuse an OpenAI `data[].id` parser or infer route eligibility from a public API pricing catalog.

## References

- [SIWC models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [SIWC registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [SIWC accounts and sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)
- [SIWC token reference](https://developers.openai.com/siwc/token-sharing-open-source/token-reference)
- [SIWC errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery)
- [SIWC preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
- [OpenAI terms of use](https://openai.com/policies/terms-of-use/)
- [GitHub Copilot user authentication](https://oceans-llm.com/providers/github-copilot/#github-user-authentication)
