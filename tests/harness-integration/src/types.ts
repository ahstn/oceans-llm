export interface GatewayRuntime {
  allowlistedUser?: {
    apiKey: string;
    model: string;
  };
  adminEmail: string;
  adminPassword: string;
  apiKey: string;
  baseUrl: string;
  model: string;
}

export interface ToolCall {
  input: unknown;
  name: string;
}

export interface HarnessRun {
  requestTag: string;
  output: string;
  toolCalls: ToolCall[];
}

export interface HarnessAdapter {
  readonly key: "pi" | "opencode";
  readonly label: string;
  run(workspace: string, prompt: string): Promise<HarnessRun>;
}

declare module "vitest" {
  export interface ProvidedContext {
    gateway: GatewayRuntime;
  }
}
