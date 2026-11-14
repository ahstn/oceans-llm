/**
 * Request-log `agent_harness_key` values of the harnesses that this suite drives. The gateway
 * classifies Claude Code model requests (`claude-cli/<version>`) as `claude_cli`.
 */
export type HarnessKey = "opencode" | "pi" | "oh_my_pi" | "claude_cli";

/** Request-log `metadata.operation` values of the inference endpoints. */
export type Operation = "chat_completions" | "messages" | "responses";

/** A matrix model family. */
export type ModelFamily = "openai" | "anthropic" | "google";

/** The upstream provider of a matrix cell. */
export type Provider = "openrouter" | "vertex" | "bedrock";

export interface GatewayRuntime {
  allowlistedUser?: {
    apiKey: string;
    model: string;
  };
  adminEmail: string;
  adminPassword: string;
  apiKey: string;
  baseUrl: string;
  /** OpenRouter model keys, which the plugin suite also uses. */
  pluginModels: {
    openai: string;
    anthropic: string;
    google: string;
    textOnly: string;
    hidden: string;
  };
  /** Model keys on other providers. A provider without keys is not configured, so its cells skip. */
  providerModels: Partial<Record<Exclude<Provider, "openrouter">, Partial<Record<ModelFamily, string>>>>;
}

export interface ToolCall {
  input: unknown;
  name: string;
  status: "started" | "completed" | "error";
}

export interface HarnessRun {
  requestTag: string;
  output: string;
  toolCalls: ToolCall[];
}

export interface HarnessRunOptions {
  /**
   * Connects the harness to the Oceans aggregate MCP endpoint. Runs without it skip MCP startup,
   * so a Context7 outage cannot fail them.
   */
  mcp?: boolean;
}

/** Native tool names in the harness's event stream. */
export interface HarnessTools {
  read: string;
  shell: string;
  /** Tools that can create a file. OpenCode gives GPT models `patch` in place of `write`. */
  write: readonly string[];
}

export interface HarnessAdapter {
  readonly harness: HarnessKey;
  readonly label: string;
  readonly tools: HarnessTools;
  run(
    workspace: string,
    prompt: string,
    model: string,
    options?: HarnessRunOptions,
  ): Promise<HarnessRun>;
}

declare module "vitest" {
  export interface ProvidedContext {
    gateway: GatewayRuntime;
  }
}
