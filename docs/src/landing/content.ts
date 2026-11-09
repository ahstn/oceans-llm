/**
 * Every word and data point on the landing page, shared by the page
 * (src/pages/index.astro) and its Markdown twin (src/pages/index.md.ts).
 *
 * Claims here must match the docs. Each block names the page it comes from,
 * so a change to the product has one place to update.
 */
import type { ImageMetadata } from "astro";

import shotRequestLogs from "@/assets/landing/shot-request-logs.webp";
import shotModels from "@/assets/landing/shot-models.webp";
import shotApiKeys from "@/assets/landing/shot-api-keys.webp";
import shotSpend from "@/assets/landing/shot-spend.webp";
import cropSpendRows from "@/assets/landing/crop-spend-rows.webp";
import cropClientConfig from "@/assets/landing/crop-client-config.webp";
import cropOpencodeDialog from "@/assets/landing/crop-opencode-dialog.webp";
import cropEditUser from "@/assets/landing/crop-edit-user.webp";
import cropLeaderboardTip from "@/assets/landing/crop-leaderboard-tip.webp";
import cropHarnessChart from "@/assets/landing/crop-harness-chart.webp";
import cropSessionsRows from "@/assets/landing/crop-sessions-rows.webp";
import cropMcpTools from "@/assets/landing/crop-mcp-tools.webp";
import cropMcpGrant from "@/assets/landing/crop-mcp-grant.webp";
import stageBands from "@/assets/landing/stage-bands.webp";
import stageCurl from "@/assets/landing/stage-curl.webp";

export const GITHUB_URL = "https://github.com/ahstn/oceans-llm";
export const LICENSE_URL = `${GITHUB_URL}/blob/main/LICENSE`;

export interface Link {
  label: string;
  href: string;
}

/** A run of text; a run with `href` is a link, one with `code` is inline code. */
export interface Run {
  text: string;
  href?: string;
  code?: boolean;
}

export interface Shot {
  src: ImageMetadata;
  alt: string;
}

/** A screenshot that floats over the main one, placed by `class`. */
export interface FloatShot extends Shot {
  class: string;
}

export const meta = {
  title: "Oceans LLM",
  description:
    "One gateway for your organisation's AI traffic. Route model and MCP requests, govern who can use what, control spend, and see where usage goes.",
};

// ---- Hero (README.md, LICENSE, setup/index.mdx) ----

export const hero = {
  eyebrow: "Open source AI gateway",
  license: "AGPL-3.0",
  title: "Every model. Every agent.",
  subtitle: "One gateway.",
  lede: "Point your coding agents and apps at one endpoint. Oceans LLM routes their model and MCP calls, controls who can use what, caps spend, protects against misalignment, and logs every request.",
  primary: { label: "Get started", href: "/setup/" },
  secondary: { label: "Read the docs", href: "/introduction/" },
  command: "mise run dev-stack",
  commandNote: "Starts the gateway and admin UI from a clone of the repo.",
  shot: {
    title: "Request logs",
    main: {
      src: shotRequestLogs,
      alt: "Request logs page in the Oceans admin UI, listing each request's model, caller, status, cost, latency and tokens",
    },
    floats: [
      {
        src: cropSpendRows,
        alt: "Two budget rows: one user over a $200 monthly hard limit, another near a $150 limit",
        class: "left-[2%] bottom-[9%] w-[41%] xl:-left-[4%]",
      },
      {
        src: cropClientConfig,
        alt: "Generated Claude Code settings.json that points Claude Code at the gateway",
        class: "right-[2%] top-[22%] w-[44%] xl:-right-[5%]",
      },
    ] satisfies FloatShot[],
  },
};

// ---- Statement (introduction.mdx) ----

export const statement: Run[] = [
  { text: "Coding agents and backend applications call " },
  { text: "one API", href: "/configuration/model-routing/" },
  { text: " with an Oceans key. Admins decide, in one place, which " },
  { text: "models and tools", href: "/access/identity-and-access/" },
  { text: " each caller can reach, " },
  { text: "how much they can spend", href: "/spend/budgets/" },
  { text: ", and " },
  { text: "what gets logged", href: "/observability/request-logs/" },
  { text: "." },
];

export const audiences = [
  {
    title: "For admins",
    icon: "ph:shield-check",
    items: [
      "Register providers, models and MCP servers once",
      "Grant models and tools to users, teams, service accounts and keys",
      "Set daily, weekly or monthly budgets with hard limits",
      "Review request logs, agent traffic and spend in one admin UI",
    ],
  },
  {
    title: "For users and callers",
    icon: "ph:terminal-window",
    items: [
      "One API key for every model and MCP you're granted",
      "Generated config snippets and plugins for Claude Code, Codex, Pi, etc",
      "One /mcp URL for every tool you're allowed to call",
      "Your own spend, exported as CSV",
    ],
  },
];

// ---- Request lifecycle (reference/request-lifecycle.mdx) ----

export const lifecycle = {
  eyebrow: "How it works",
  title: "Every request takes the same path.",
  lede: "A call from a coding agent and a call from a CI job go through the same checks, in the same order. Then the gateway records what happened.",
  link: { label: "Request lifecycle and failure modes", href: "/reference/request-lifecycle/" },
  steps: [
    { title: "Authenticate", body: "Check the API key and its scope." },
    { title: "Resolve", body: "Check model access and resolve aliases." },
    { title: "Plan", body: "Pick a route that can serve the request, then apply rate limits." },
    { title: "Guard", body: "Run prompt guardrails, then the budget gate." },
    { title: "Call", body: "Send one attempt to the provider. There are no hidden retries." },
    { title: "Record", body: "Charge usage, check the response, and write the request log." },
  ],
};

/** Colour per lifecycle step, from the Tokyo Night data set in DESIGN.md. */
export const stepColors = ["#7aa2f7", "#7dcfff", "#41a6b5", "#9d7cd8", "#9ece6a", "#b5dde5"];

/**
 * Spans of one example `POST /v1/responses` trace, in seconds. Span names are
 * the gateway's own (crates/gateway-service, crates/gateway/src/http).
 * `step` is the lifecycle step, 1-based; 0 is the root span.
 */
export const trace = {
  method: "POST",
  path: "/v1/responses",
  status: 200,
  total: 4.77,
  caption: "An example trace for one request, exported over OTLP.",
  link: { label: "Export traces and metrics", href: "/observability/export-traces-and-metrics/" },
  spans: [
    { name: "POST /v1/responses", step: 0, start: 0, duration: 4.77, label: "4.77 s" },
    { name: "gateway.auth.authenticate", step: 1, start: 0.121, duration: 0.0264, label: "26.4 ms" },
    { name: "gateway.request.resolve", step: 2, start: 0.15, duration: 0.00582, label: "5.82 ms" },
    { name: "gateway.route.plan", step: 3, start: 0.16, duration: 0.0000116, label: "11.6 µs" },
    { name: "gateway.budget.precheck", step: 4, start: 0.175, duration: 0.0027, label: "2.70 ms" },
    { name: "gateway.provider.operation", step: 5, start: 0.179, duration: 1.17, label: "1.17 s" },
    { name: "gateway.provider.stream", step: 5, start: 0.179, duration: 4.54, label: "4.54 s" },
    { name: "gateway.usage.accounting", step: 6, start: 4.72, duration: 0.0135, label: "13.5 ms" },
    { name: "gateway.request_log.persist", step: 6, start: 4.733, duration: 0.0368, label: "36.8 ms" },
  ],
};

// ---- Feature pillars (features.mdx and the pages each links to) ----

export interface Pillar {
  id: string;
  icon: string;
  eyebrow: string;
  title: string;
  body: Run[];
  points: string[];
  link: Link;
  stage: {
    backdrop: ImageMetadata;
    /** Mirror the backdrop so neighbouring stages don't repeat. */
    flip?: boolean;
    /** `window` gets app chrome and bleeds off the stage; `panel` sits inside it. */
    kind: "window" | "panel";
    title?: string;
    main: Shot;
    floats: FloatShot[];
  };
}

export const pillars: Pillar[] = [
  {
    id: "routing",
    icon: "ph:path",
    eyebrow: "Routing and clients",
    title: "One API in front of every provider.",
    body: [
      { text: "Callers use OpenAI-compatible Chat Completions, Responses and embeddings, Anthropic Messages with token counting, and image generation. Admins map model aliases to provider routes in one config file." },
    ],
    points: [
      "Seven provider types, from OpenAI-compatible APIs to Vertex AI, Bedrock and GitHub Copilot",
      "Capability checks pick a route that can serve each request",
      "Client config generated from the live model catalog, so nobody types model data by hand",
    ],
    link: { label: "Model routing and APIs", href: "/configuration/model-routing/" },
    stage: {
      backdrop: stageBands,
      kind: "window",
      title: "Models",
      main: {
        src: shotModels,
        alt: "Models page listing gateway models with their provider routes and Intelligence Index scores",
      },
      floats: [
        {
          src: cropOpencodeDialog,
          alt: "Client config dialog with tabs for OpenCode, Pi and Claude Code, showing a generated opencode.json",
          class: "left-[5%] bottom-[6%] w-[54%]",
        },
      ],
    },
  },
  {
    id: "identity",
    icon: "ph:fingerprint",
    eyebrow: "Identity and access",
    title: "Know who can use what.",
    body: [
      { text: "People sign in with a password, GitHub, or your OIDC provider. Every API key belongs to a user or a service account, and you can limit it to certain models." },
    ],
    points: [
      "Users, teams and service accounts are separate identities",
      "Team model limits apply on top of each key's own",
      "Revoke one key without touching any other",
    ],
    link: { label: "Identity and access", href: "/access/identity-and-access/" },
    stage: {
      backdrop: stageCurl,
      flip: true,
      kind: "window",
      title: "API keys",
      main: {
        src: shotApiKeys,
        alt: "API keys page listing each key's owner, granted models, scopes and status",
      },
      floats: [
        {
          src: cropEditUser,
          alt: "User dialog showing a user's global role, team, team role, sign-in method and request logging",
          class: "right-[4%] bottom-[6%] w-[50%]",
        },
      ],
    },
  },
  {
    id: "spend",
    icon: "ph:coins",
    eyebrow: "Spend",
    title: "Budgets that stop spend, not just report it.",
    body: [
      { text: "Set daily, weekly or monthly budgets for users and service accounts. Turn on a hard limit to block new requests once a budget is spent." },
    ],
    points: [
      "Model budgets let a user try a frontier model without risking their whole allowance",
      "An alert when 20% or less of a budget is left",
      "Costs priced from an effective-dated catalog, with the price source recorded",
    ],
    link: { label: "Budgets", href: "/spend/budgets/" },
    stage: {
      backdrop: stageBands,
      flip: true,
      kind: "window",
      title: "Spend controls",
      main: {
        src: shotSpend,
        alt: "Spend controls page with spend against budgets, owners over budget or near their limit, and per-user usage bars",
      },
      floats: [
        {
          src: cropLeaderboardTip,
          alt: "Leaderboard tooltip with the five users with the highest cost on one day",
          class: "left-[6%] bottom-[8%] w-[34%]",
        },
      ],
    },
  },
  {
    id: "observability",
    icon: "ph:binoculars",
    eyebrow: "Observability",
    title: "See every request, and the agent behind it.",
    body: [
      { text: "Request logs show each request's owner, model, provider, latency, tokens and cost. Harness tracking shows which coding agents send the traffic." },
    ],
    points: [
      "Agent sessions show the cost, active time and tool calls of each piece of work",
      "OTLP traces and metrics for the observability stack you already run",
      "Request bodies stored redacted by default, or not at all",
    ],
    link: { label: "Request logs", href: "/observability/request-logs/" },
    stage: {
      backdrop: stageCurl,
      kind: "panel",
      main: {
        src: cropHarnessChart,
        alt: "Agent harnesses chart comparing request activity from Codex, OpenCode, Claude Code, Mastra and Oh My Pi",
      },
      floats: [
        {
          src: cropSessionsRows,
          alt: "Agent sessions table with each session's harness, model, cost, active time and request count",
          class: "right-[5%] bottom-[7%] w-[78%]",
        },
      ],
    },
  },
  {
    id: "mcp",
    icon: "ph:plugs-connected",
    eyebrow: "MCP gateway",
    title: "One MCP endpoint, scoped to each caller.",
    body: [
      { text: "Register an MCP server once. Clients connect to " },
      { text: "/mcp", code: true },
      { text: " and find their granted tools with " },
      { text: "search_tools", code: true },
      { text: ", " },
      { text: "describe_tool", code: true },
      { text: " and " },
      { text: "call_tool", code: true },
      { text: ", so tool lists and context windows stay small." },
    ],
    points: [
      "Grant single tools or toolsets to a key, user, team or service account",
      "Tag rules allow or deny tools by what they do, and exclude rules always win",
      "Changed tools can wait for an admin's approval before clients see them",
    ],
    link: { label: "MCP servers", href: "/extensions/mcp-servers/" },
    stage: {
      backdrop: stageBands,
      kind: "window",
      title: "MCP servers",
      main: {
        src: cropMcpTools,
        alt: "MCP server dialog showing a discovered tool with its upstream hints, access tags and annotation overrides",
      },
      floats: [
        {
          src: cropMcpGrant,
          alt: "New grant form that assigns a tool or toolset to a user",
          class: "left-[5%] bottom-[6%] w-[50%]",
        },
      ],
    },
  },
];

// ---- Harnesses (harnesses/*.mdx, observability/agent-harness-usage.mdx) ----

export const harnessSection = {
  eyebrow: "Agents",
  title: "Bring the agents your team already uses.",
  lede: "Point each harness at the gateway with one API key. The admin UI writes the config from the live catalog. OMP and the OpenCode plugin find your models by themselves.",
  link: { label: "Client harness configuration", href: "/harnesses/" },
  trackedLabel: "Recognised from the User-Agent header",
  tracked: [
    "Claude Code",
    "Codex",
    "Opencode",
    "Pi",
    "Oh My Pi",
    "Gemini CLI",
    "Copilot CLI",
    "Claude CLI",
    "GitHub Copilot",
    "Mastra",
    "DSPy",
    "curl",
  ],
  trackedLink: { label: "Agent harness usage", href: "/observability/agent-harness-usage/" },
};

export interface HarnessSnippet {
  id: string;
  label: string;
  note: string;
  link: Link;
  files: { path: string; lang: "json" | "jsonc" | "toml" | "yaml"; code: string }[];
}

export const harnesses: HarnessSnippet[] = [
  {
    id: "claude-code",
    label: "Claude Code",
    note: "Generated by the admin UI for the models you pick.",
    link: { label: "Client harness configuration", href: "/harnesses/" },
    files: [
      {
        path: "~/.claude/settings.json",
        lang: "json",
        code: `{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "<gateway api token>",
    "ANTHROPIC_BASE_URL": "https://api.oceans-llm.com",
    "ANTHROPIC_MODEL": "claude-sonnet",
    "ANTHROPIC_SMALL_FAST_MODEL": "claude-sonnet",
    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY": "1"
  }
}`,
      },
    ],
  },
  {
    id: "codex",
    label: "Codex",
    note: "Codex talks to the gateway with the Responses API.",
    link: { label: "Client harness configuration", href: "/harnesses/" },
    files: [
      {
        path: "~/.codex/config.toml",
        lang: "toml",
        code: `model = "<gateway model id>"
model_provider = "oceans-llm"

[model_providers.oceans-llm]
name = "oceans-llm"
base_url = "https://api.oceans-llm.com/v1"
env_key = "OCEANS_LLM_API_KEY"
requires_openai_auth = false
wire_api = "responses"`,
      },
    ],
  },
  {
    id: "opencode",
    label: "OpenCode",
    note: "The plugin reads the model list from the gateway and keeps it current.",
    link: { label: "OpenCode", href: "/harnesses/opencode/" },
    files: [
      {
        path: "~/.config/opencode/opencode.jsonc",
        lang: "jsonc",
        code: `{
  "$schema": "https://opencode.ai/config.json",
  "plugins": ["@ahstn/opencode-oceans@0.1.0"],
}`,
      },
      {
        path: "~/.config/opencode/gateways.yml",
        lang: "yaml",
        code: `providers:
  oceans:
    name: Oceans
    baseUrl: https://api.oceans-llm.com
    apiKey: OCEANS_LLM_API_KEY
    discovery:
      type: litellm`,
      },
    ],
  },
  {
    id: "pi",
    label: "Pi",
    note: "Generated by the admin UI, with prices and context windows from the catalog.",
    link: { label: "Client harness configuration", href: "/harnesses/" },
    files: [
      {
        path: "~/.pi/agent/models.json",
        lang: "json",
        code: `{
  "providers": {
    "oceans-llm": {
      "api": "anthropic-messages",
      "apiKey": "$OCEANS_LLM_API_KEY",
      "baseUrl": "https://api.oceans-llm.com",
      "models": [
        { "id": "claude-sonnet", "contextWindow": 200000, "reasoning": true }
      ]
    }
  }
}`,
      },
    ],
  },
  {
    id: "omp",
    label: "OMP",
    note: "Oh-My-Pi finds models, prices and reasoning levels with its own discovery.",
    link: { label: "Oh-My-Pi (OMP)", href: "/harnesses/omp/" },
    files: [
      {
        path: "~/.omp/agent/models.yml",
        lang: "yaml",
        code: `providers:
  oceans:
    baseUrl: https://api.oceans-llm.com/v1
    apiKey: OCEANS_LLM_API_KEY
    api: openai-completions
    discovery:
      type: litellm`,
      },
    ],
  },
];

// ---- Details bento (features.mdx and the linked pages) ----

export type BentoKind = "sso" | "tags" | "limits" | "guardrails" | "focus" | "audit";

export interface BentoItem {
  kind: BentoKind;
  icon: string;
  title: string;
  body: string;
  link: Link;
}

export const bentoSection = {
  eyebrow: "The details",
  title: "The parts admins ask about first.",
};

export const bento: BentoItem[] = [
  {
    kind: "sso",
    icon: "ph:sign-in",
    title: "Sign in your way",
    body: "Password, GitHub OAuth, or an OIDC provider such as Google, Okta or Authentik.",
    link: { label: "OIDC and SSO", href: "/access/sso/" },
  },
  {
    kind: "guardrails",
    icon: "ph:shield-warning",
    title: "Guardrails for agents",
    body: "14 built-in packs catch destructive shell, Git, cloud and database operations. Start in audit mode, then deny.",
    link: { label: "Gateway guardrails", href: "/operations/gateway-guardrails/" },
  },
  {
    kind: "limits",
    icon: "ph:gauge",
    title: "Rate limits that explain themselves",
    body: "Cap requests or tokens per minute, hour, day or month. Responses say how much is left.",
    link: { label: "Rate limits", href: "/spend/rate-limits/" },
  },
  {
    kind: "tags",
    icon: "ph:tag",
    title: "Tags for attribution",
    body: "Callers tag each request, so usage maps back to cost centres, workloads and environments.",
    link: { label: "Tagging", href: "/observability/tagging/" },
  },
  {
    kind: "focus",
    icon: "ph:file-csv",
    title: "FinOps-ready export",
    body: "Export usage as best-effort FOCUS-compatible CSV, for the whole gateway or just your own spend.",
    link: { label: "Budgets", href: "/spend/budgets/" },
  },
  {
    kind: "audit",
    icon: "ph:clipboard-text",
    title: "An audit log for admin changes",
    body: "Changes to MCP servers, grants, credentials and rate limits record who made them and what changed.",
    link: { label: "Admin audit log", href: "/access/admin-audit-log/" },
  },
];

// ---- Deployment (setup/*.mdx) ----

export const deploySection = {
  eyebrow: "Deployment",
  title: "Run it where your data lives.",
  lede: "Start on a laptop with a libsql file. Move to PostgreSQL on Docker Compose, Kubernetes or a single host when you're ready.",
  link: { label: "Choose how to run Oceans", href: "/setup/" },
};

export const deployments = [
  {
    icon: "ph:laptop",
    title: "Local",
    store: "libsql",
    body: "Sign in at localhost:8080/admin as admin@local.",
    command: "mise run dev-stack",
    link: { label: "Start up and first access", href: "/setup/first-access/" },
  },
  {
    icon: "ph:shipping-container",
    title: "Docker Compose",
    store: "PostgreSQL",
    body: "Fill in deploy/.env first: secrets, the public URL and provider keys.",
    command: "cp deploy/.env.example deploy/.env\ndocker compose -f deploy/compose.yaml up -d",
    link: { label: "Start up and first access", href: "/setup/first-access/" },
  },
  {
    icon: "ph:stack",
    title: "Kubernetes",
    store: "PostgreSQL",
    body: "Install the published chart, with migrations as hook Jobs.",
    command: "helm install oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \\\n  --version <version> --values values.yaml",
    link: { label: "Kubernetes and Helm", href: "/setup/kubernetes-and-helm/" },
  },
  {
    icon: "ph:hard-drives",
    title: "Single host",
    store: "PostgreSQL",
    body: "One Linux machine with mise daemons, reachable over Tailscale.",
    command: "mise run deploy",
    link: { label: "Self-hosting with mise", href: "/setup/self-hosting/" },
  },
];

// ---- FAQ ----

export const faq: { question: string; answer: Run[] }[] = [
  {
    question: "Is Oceans LLM open source?",
    answer: [
      { text: "Yes. It's licensed under " },
      { text: "AGPL-3.0", href: LICENSE_URL },
      { text: ", and you run it on your own infrastructure." },
    ],
  },
  {
    question: "Which APIs can callers use?",
    answer: [
      { text: "OpenAI-compatible Chat Completions, Responses, embeddings and image generation, plus Anthropic Messages with token counting. MCP clients connect to " },
      { text: "/mcp", code: true },
      { text: ". See " },
      { text: "Model routing and APIs", href: "/configuration/model-routing/" },
      { text: "." },
    ],
  },
  {
    question: "What happens when a provider fails?",
    answer: [
      { text: "The gateway makes one attempt and returns the error. It doesn't retry or fall back to another route, so the request log shows what really happened. See " },
      { text: "Request lifecycle and failure modes", href: "/reference/request-lifecycle/" },
      { text: "." },
    ],
  },
  {
    question: "Does it store prompts?",
    answer: [
      { text: "By default, request logs keep redacted request and response bodies. Set " },
      { text: "capture_mode", code: true },
      { text: " to " },
      { text: "summary_only", code: true },
      { text: " to keep no bodies, or to " },
      { text: "disabled", code: true },
      { text: " to keep no request logs. See " },
      { text: "Request logs", href: "/observability/request-logs/" },
      { text: "." },
    ],
  },
  {
    question: "Can a team have a budget?",
    answer: [
      { text: "No, on purpose. Teams group users and own things, such as service accounts and MCP servers, but they don't spend. Each request is charged to the user or service account that made it. See " },
      { text: "Budgets", href: "/spend/budgets/" },
      { text: "." },
    ],
  },
  {
    question: "Does a hard limit stop spend exactly at the cap?",
    answer: [
      { text: "Not quite. A hard limit blocks new requests once recorded spend reaches the cap, but requests that are already running can take spend past it." },
    ],
  },
];

// ---- Closing call to action and footer ----

export const closing = {
  title: "Put one gateway in front of it all.",
  lede: "Start on your laptop, then deploy with Compose, Helm or mise.",
  primary: { label: "Get started", href: "/setup/" },
  secondary: { label: "View on GitHub", href: GITHUB_URL },
};

export const footerColumns: { title: string; links: Link[] }[] = [
  {
    title: "Product",
    links: [
      { label: "Features", href: "/features/" },
      { label: "Choose how to run Oceans", href: "/setup/" },
      { label: "Client harness configuration", href: "/harnesses/" },
      { label: "API reference", href: "/api/" },
    ],
  },
  {
    title: "Guides",
    links: [
      { label: "Budgets", href: "/spend/budgets/" },
      { label: "Request logs", href: "/observability/request-logs/" },
      { label: "MCP servers", href: "/extensions/mcp-servers/" },
      { label: "OIDC and SSO", href: "/access/sso/" },
    ],
  },
  {
    title: "Project",
    links: [
      { label: "GitHub", href: GITHUB_URL },
      { label: "Licence", href: LICENSE_URL },
      { label: "llms.txt", href: "/llms.txt" },
    ],
  },
];
