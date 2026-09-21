# TypeSafe

`See also`: [OpenRouter](openrouter.md), [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

Oceans exposes TypeSafe System One models through `POST /v1/decisions`. A request sends one `state` value and a map of typed `questions`. The response returns one structured answer for each question.

Jev does not generate chat text. Do not send it to Chat Completions or Responses.

## Native TypeSafe Provider

Use `type: typesafe` to call TypeSafe directly. The adapter sends requests to `{base_url}/v1/systemone`. When `base_url` already ends in `/v1`, it appends only `/systemone`. The base URL must be a bare origin or end in `/v1`. It must use HTTPS, except that HTTP is allowed for loopback addresses during local development.

```yaml
providers:
  - id: typesafe
    type: typesafe
    base_url: https://api.typesafe.ai
    pricing_provider_id: openrouter
    auth:
      kind: bearer
      token: env.TYPESAFE_API_KEY
    display:
      label: TypeSafe
      icon_key: typesafe

models:
  - id: jev-native
    description: TypeSafe Jev through the native System One API
    routes:
      - provider: typesafe
        upstream_model: jev-latest
        context_window_tokens: 32000
        pricing_override:
          input_usd_per_million_tokens: "0.0420"
          output_usd_per_million_tokens: "0.0000"
        capabilities:
          chat_completions: false
          responses: false
          stream: false
          embeddings: false
          decisions: true
          tools: false
          vision: false
          json_schema: false
          developer_role: false
```

`pricing_provider_id` must name a supported catalog family. The native model alias `jev-latest` does not have an exact OpenRouter catalog identity, so the example uses a route pricing override. The checked-in OpenRouter route uses `typesafe/jev-1.13`, which has fallback catalog metadata for a 32,000-token context and rates of $0.042 input and $0 output per million tokens.

## Request Contract

Send the stable gateway model ID, not the provider model ID:

```http
POST /v1/decisions
Authorization: Bearer <OCEANS_API_KEY>
Content-Type: application/json
```

```json
{
  "model": "jev-native",
  "state": "Help! My payouts have been failing for 3 days.",
  "questions": {
    "is_urgent": {
      "type": "noul",
      "instructions": "Does this convey urgency?"
    },
    "department": {
      "type": "choice",
      "instructions": "Which team should handle this?",
      "criteria": {
        "billing": "Payments, invoicing, and refunds",
        "technical": "Bugs, outages, and integrations"
      }
    },
    "frustration": {
      "type": "score",
      "instructions": "How frustrated is the customer?",
      "criteria": ["Calm", "Frustrated", "Very angry"]
    }
  }
}
```

Question rules:

- `noul` asks a yes/no question and returns a probability from 0 to 1. Its optional `criteria` can describe `true` and `false`.
- `choice` selects one named option. It requires 1 to 255 criteria entries.
- `score` returns a probability-weighted score over 2 to 10 ordered levels.
- `instructions`, descriptions, and levels can contain JSON strings, objects, or arrays.

Oceans requires a non-empty model, at least one question, valid choice and score bounds, and an answer for every requested question. Each answer type must match its question type. Other provider fields pass through unless a route `extra_body` value overrides them.

## Runtime Behavior

- Decisions requests use the normal gateway authentication, model access, route selection, budget, request-log, provider-attempt, and usage-accounting paths.
- Decisions routes skip prompt and response guardrails. The guardrail evaluators currently operate on chat-shaped text and tool-call boundaries.
- Decisions are non-streaming.
- Provider `input_tokens` and `output_tokens` normalize to the gateway's prompt and completion token fields. When both are present, Oceans derives total tokens.
- The Models page displays a **Decisions** capability and does not offer chat-shaped client configuration for a decisions-only model.
- Request Logs display the operation as **Decisions**.

For OpenRouter transport and route configuration, see [OpenRouter](openrouter.md#decisions-api).
