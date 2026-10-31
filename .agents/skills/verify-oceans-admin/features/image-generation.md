# Image generation

Image generation lets a caller create images through the gateway with the same key, grant, budget, rate-limit and logging rules as text models. Image-only models such as GPT Image use the Images API (`POST /v1/images` and `POST /v1/images/generations`). Chat image models such as Gemini image models use Chat Completions with `modalities: ["image","text"]`. The gateway records the provider-reported cost for image output, and request logs keep the response structure while each base64 image and reasoning blob becomes `{truncated, size_bytes, preview}`.

## Sub-features

- `images-api` sends one Images API request with `gpt-image-2.5-flare` and gets `data[0].b64_json` back.
- `images-alias` sends the same request to `/v1/images/generations` and gets the same response shape.
- `images-grant` confirms that a key without a grant for `gpt-image-2.5-flare` gets the usual 403 on `/v1/images`.
- `images-chat` sends one Chat Completions request with `modalities: ["image","text"]` to a Gemini image model and gets `choices[0].message.images[0].image_url.url` back.
- `images-cost` confirms that the recorded cost matches the provider-reported cost within 1%: `usage.cost`, plus `usage.cost_details.upstream_inference_cost` when `usage.is_byok` is true.
- `images-request-log` confirms that the request log keeps `usage` and the JSON structure, with each image and `reasoning_details[].data` replaced by `{truncated, size_bytes, preview}`.
- `images-models` confirms that `/v1/models` lists `image` in `output_modalities` for `gpt-image-2.5-flare`.
- `images-key-cleanup` revokes both temporary gateway keys and confirms that gateway authentication rejects the first.

## How to get to it (user POV)

- Sign in, choose `API Keys`, and create a unique temporary key with access to `gpt-image-2.5-flare` and one Gemini image model.
- Call `POST /v1/images` or `POST /v1/images/generations` with `model: gpt-image-2.5-flare` and a short synthetic `prompt`.
- Call `POST /v1/chat/completions` with `model: gemini-nano-banana-2.1` (or `gemini-3.1-flash-lite-image`), `modalities: ["image","text"]` and a short synthetic prompt.
- Choose `Request Logs` under `Observability` and open the matching requests.
- Call `GET /v1/models` with the temporary key.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `OPENROUTER_API_KEY` is in the launch environment. If it is not set, `source ~/.config/zsh/.zsh-private` before `launch`.
- The operator accepts three short paid requests: two Images API calls (about $0.007 each at `quality: low`) and one chat image call (about $0.035). The 403 check and `/v1/models` are free.
- Create a unique temporary gateway API key through the API Keys recipe with only the `inference` capability and grants for `gpt-image-2.5-flare`, `gemini-nano-banana-2.1` and `gemini-3.1-flash-lite-image`, so the chat fallback below needs no grant change. Create a second temporary key with a grant for `deepseek-v4.1-flash` only, for the 403 check. Keep both keys in memory or only in the current shell. Do not write them to evidence or command history.

- **Images API.** Send `POST /v1/images` with `{"model":"gpt-image-2.5-flare","prompt":"A small red circle on a white background","quality":"low"}` and a unique `x-request-id`. Expect 200, a non-empty `data[0].b64_json` (a base64 image), and `usage` with `completion_tokens_details.image_tokens`. `data[0].media_type` is optional; when present, it must be an `image/` type. Save only the status, the `b64_json` length and the usage numbers, not the image.
- **Alias.** Send the same body to `POST /v1/images/generations`. Expect the same response shape.
- **Grant.** Send the same body to `POST /v1/images` with the second key. Expect 403 with the standard OpenAI error envelope and no provider attempt.
- **Chat image.** Send `POST /v1/chat/completions` with `{"model":"gemini-nano-banana-2.1","modalities":["image","text"],"messages":[{"role":"user","content":"Create a simple flat illustration of a blue paper boat on calm water."}]}`. Expect 200 and `choices[0].message.images[0].image_url.url` as a `data:image/...;base64,` URL. If the upstream returns 429 (Google AI Studio quota for Nano Banana), repeat once with `gemini-3.1-flash-lite-image` and record the 429 as a `blocked` optional check.
- **Cost.** For each successful call, compute the provider cost from the response `usage`: `cost`, plus `cost_details.upstream_inference_cost` when `is_byok` is true. Open the request in `Request Logs` and read the `Cost` tile (`request-log-key-metrics`). Require a match within 1%, or within $0.0001 when the cost is below $0.01, because the ledger keeps four decimal places.
- **Request log.** In the request detail, check that the `Tokens` tile shows the usage. Through `GET /api/v1/admin/observability/request-logs/{request_log_id}`, check that the stored response payload is a JSON object (not a single truncated string), that it keeps `usage`, and that `data[0].b64_json` (Images API), `choices[0].message.images[0].image_url.url` and every `choices[0].message.reasoning_details[].data` (chat) are objects with `truncated: true`, `size_bytes` and `preview`. Record only key names and sizes.
- **Models.** Call `GET /v1/models` with the first key. Expect the `gpt-image-2.5-flare` entry to have `architecture.output_modalities` that contains `image`.
- **Key cleanup.** Revoke both temporary keys and confirm that the first can no longer call `/v1/models`.
- **Proof.** Record with `control-oceans-admin record image-generation`. Use one `--check` each for `images-api`, `images-alias`, `images-grant`, `images-chat`, `images-cost`, `images-request-log`, `images-models` and `images-key-cleanup`, with the gateway model and request-log ID in the notes. Attach the Request Logs list and detail screenshots as `--artifact`, with `request-log-payloads` masked. Then run the normal stack cleanup and `control-oceans-admin evidence image-generation`.

## Gotchas

- OpenRouter serves GPT Image only on `POST /api/v1/images`. A Chat Completions call to `gpt-image-2.5-flare` gets no compatible route in the gateway, because its route serves only Images: the OpenRouter catalog lists the model with image output only, so catalog hints turn `images` on and Chat Completions, Responses, and Embeddings off, and `gateway.yaml` explicitly turns `stream`, `vision`, `json_schema`, and `developer_role` off. The model's `Capabilities` in Models shows the effective result.
- The Images API is non-streaming in the gateway. A body with `stream: true` returns 400.
- With a BYOK provider key, OpenRouter reports `cost: 0` and puts the provider bill in `cost_details.upstream_inference_cost`. The gateway records the sum, so the `Cost` tile is not zero.
- Image responses run to 1–2.5 MB. Do not print raw response bodies; they hold the image and encrypted reasoning.
- `gemini-nano-banana-2.1` has one upstream provider (Google AI Studio) and often returns 429 from a shared quota. That is an upstream limit, not a gateway failure. `gemini-3.1-flash-lite-image` uses the same response format.
- Google can also return 200 with `finish_reason: content_filter` (`native_finish_reason: IMAGE_RECITATION`), no image, and no `usage`, even for a plain shape prompt. The ledger then records `usage_missing` with no cost. Retry once with a different synthetic prompt, such as a simple flat illustration of a paper boat.
- The ledger rounds cost to four decimal places. A very small cost can round to `$0.0000` or differ by up to $0.00005.
- Never record raw API keys, authorization headers, prompts beyond the synthetic text above, or image data.
