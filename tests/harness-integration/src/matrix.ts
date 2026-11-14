import type { HarnessKey, ModelFamily, Operation, Provider } from "./types.js";

/**
 * The harness x model x provider matrix. `matrix.test.ts` generates its tests from this module,
 * and the published matrix in `docs/src/content/docs/harnesses/verification.mdx` must match it.
 */

/**
 * A matrix cell:
 * - `run`: the suite runs it whenever its provider is configured.
 * - `blocked`: it applies, but it cannot run yet, for `reason`. With `optIn`, setting the `env`
 *   variable runs it anyway, so that access can be checked again.
 * - `not-applicable`: the harness, model or provider does not support it, for `reason`.
 *
 * `operation` is the gateway API that the cell must use, as the request log records it.
 */
export type Cell =
  | { readonly status: "run"; readonly operation: Operation }
  | {
      readonly status: "blocked";
      readonly reason: string;
      readonly optIn?: { readonly env: string; readonly operation: Operation };
    }
  | { readonly status: "not-applicable"; readonly reason: string };

/** Turns on the blocked Bedrock cells, and the managed Bedrock provider. */
export const BEDROCK_OPT_IN = "OCEANS_HARNESS_BEDROCK";

export interface MatrixHarness {
  readonly harness: HarnessKey;
  readonly label: string;
  readonly cells: Readonly<Record<Provider, Readonly<Record<ModelFamily, Cell>>>>;
  /** The OpenRouter model of the harness's one Context7 aggregate-MCP run. */
  readonly mcpModel: ModelFamily;
}

export const MATRIX_PROVIDERS: readonly Provider[] = ["openrouter", "vertex", "bedrock"];
export const MATRIX_MODELS: readonly ModelFamily[] = ["openai", "anthropic", "google"];

/** The tests that run in every runnable cell. */
export const MATRIX_TESTS = ["chat", "files", "bash"] as const;
export type MatrixTest = (typeof MATRIX_TESTS)[number];

const run = (operation: Operation): Cell => ({ status: "run", operation });
const blocked = (reason: string): Cell => ({ status: "blocked", reason });
const notApplicable = (reason: string): Cell => ({ status: "not-applicable", reason });

const CLAUDE_MODELS_ONLY = notApplicable(
  "Claude Code is scoped to Claude models: Anthropic does not support Claude Code with non-Claude models through a gateway (https://code.claude.com/docs/en/llm-gateway), and the Oceans Claude Code snippet includes only Messages models.",
);
const VERTEX_NO_OPENAI = notApplicable("Vertex AI lists no OpenAI publisher models.");
const VERTEX_CLAUDE_QUOTA = blocked(
  "The Vertex AI quota for the anthropic-claude-haiku base model is 0 in global (quota request pending), and the model is not found in us-east5.",
);
const BEDROCK_NO_GEMINI = notApplicable("Bedrock offers Google Gemma 3 only, no Gemini models.");
const VERTEX = { openai: VERTEX_NO_OPENAI, anthropic: VERTEX_CLAUDE_QUOTA, google: run("chat_completions") };

/**
 * Bedrock cells of a harness that runs every model family. They expect the same gateway API as
 * the harness's OpenRouter cells; no run has confirmed that yet.
 */
function bedrock(openai: Operation, anthropic: Operation): Record<ModelFamily, Cell> {
  return {
    openai: {
      status: "blocked",
      reason:
        "Bedrock Runtime denies the account access to us.openai.gpt-6-luna, and Bedrock Mantle rejects /v1/responses for it.",
      optIn: { env: BEDROCK_OPT_IN, operation: openai },
    },
    anthropic: {
      status: "blocked",
      reason:
        "Bedrock denies the account access to us.anthropic.claude-haiku-5-5 (AccessDenied: model not available for this account).",
      optIn: { env: BEDROCK_OPT_IN, operation: anthropic },
    },
    google: BEDROCK_NO_GEMINI,
  };
}

export const MATRIX: readonly MatrixHarness[] = [
  {
    harness: "opencode",
    label: "OpenCode with the Oceans plugin",
    cells: {
      openrouter: {
        openai: run("responses"),
        anthropic: run("messages"),
        google: run("chat_completions"),
      },
      vertex: VERTEX,
      bedrock: bedrock("responses", "messages"),
    },
    mcpModel: "openai",
  },
  {
    harness: "pi",
    label: "Pi with the generated models.json",
    cells: {
      // The Pi snippet sends a GPT model on an OpenRouter route to Chat Completions.
      openrouter: {
        openai: run("chat_completions"),
        anthropic: run("messages"),
        google: run("chat_completions"),
      },
      vertex: VERTEX,
      bedrock: bedrock("chat_completions", "messages"),
    },
    mcpModel: "openai",
  },
  {
    harness: "oh_my_pi",
    label: "OMP with litellm discovery",
    cells: {
      openrouter: {
        openai: run("responses"),
        anthropic: run("messages"),
        google: run("chat_completions"),
      },
      vertex: VERTEX,
      bedrock: bedrock("responses", "messages"),
    },
    mcpModel: "openai",
  },
  {
    harness: "claude_cli",
    label: "Claude Code with the generated settings.json",
    cells: {
      openrouter: { openai: CLAUDE_MODELS_ONLY, anthropic: run("messages"), google: CLAUDE_MODELS_ONLY },
      vertex: { openai: CLAUDE_MODELS_ONLY, anthropic: VERTEX_CLAUDE_QUOTA, google: CLAUDE_MODELS_ONLY },
      bedrock: {
        openai: CLAUDE_MODELS_ONLY,
        // No opt-in: Bedrock runtime_anthropic_invoke routes cannot stream, whatever the access.
        anthropic: blocked(
          "Bedrock runtime_anthropic_invoke routes cannot stream, which Claude Code needs, and the account has no access to the model.",
        ),
        google: CLAUDE_MODELS_ONLY,
      },
    },
    mcpModel: "anthropic",
  },
];

/** How the suite runs a cell, or `undefined` for a cell that never runs. */
export function runnable(cell: Cell): { operation: Operation; optInEnv?: string } | undefined {
  if (cell.status === "run") return { operation: cell.operation };
  if (cell.status === "blocked" && cell.optIn) {
    return { operation: cell.optIn.operation, optInEnv: cell.optIn.env };
  }
  return undefined;
}

/** The cell as the docs show it: the operation, or `blocked: <reason>` / `not applicable: <reason>`. */
export function describeCell(cell: Cell): string {
  switch (cell.status) {
    case "run":
      return cell.operation;
    case "blocked":
      return cell.optIn
        ? `blocked: ${cell.reason} (set ${cell.optIn.env}=1 to run it)`
        : `blocked: ${cell.reason}`;
    case "not-applicable":
      return `not applicable: ${cell.reason}`;
  }
}
