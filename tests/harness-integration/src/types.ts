export interface GatewayRuntime {
  adminEmail: string;
  adminPassword: string;
  apiKey: string;
  baseUrl: string;
  model: string;
}

export interface HarnessRun {
  output: string;
  toolCalls: string[];
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
