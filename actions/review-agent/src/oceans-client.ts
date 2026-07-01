import type { EffectiveConfig, PullRequestIdentity, RepositoryIdentity, RunMetrics } from "./types";

interface Envelope<T> {
  data: T;
}

export interface ResolveConfigResponse {
  repository: unknown;
  pull_request_id: string;
  effective_config: EffectiveConfig;
  overrides_applied: unknown;
  overrides_rejected: unknown;
  reporting: unknown;
}

export interface ActionRunView {
  id: string;
  status: string;
}

export class OceansClient {
  constructor(
    private readonly baseUrl: string,
    private readonly apiKey: string,
    private readonly fetchImpl: typeof fetch = fetch,
    private readonly timeoutMs = 30_000
  ) {}

  resolveConfig(input: {
    eventName: string;
    repository: RepositoryIdentity;
    pullRequest: PullRequestIdentity;
    overrides: Record<string, unknown>;
  }): Promise<ResolveConfigResponse> {
    return this.post("/api/v1/review-agent/action/config/resolve", {
      event_name: input.eventName,
      repository: input.repository,
      pull_request: input.pullRequest,
      overrides: input.overrides
    });
  }

  startRun(input: {
    eventName: string;
    repository: RepositoryIdentity;
    pullRequest: PullRequestIdentity;
    githubRunId?: string;
    githubRunAttempt?: number;
    effectiveConfig: EffectiveConfig;
  }): Promise<{ run: ActionRunView }> {
    return this.post("/api/v1/review-agent/action/runs", {
      event_name: input.eventName,
      repository: input.repository,
      pull_request: input.pullRequest,
      github_run_id: input.githubRunId,
      github_run_attempt: input.githubRunAttempt,
      model_execution_mode: stringOrNull(input.effectiveConfig.model_execution_mode),
      provider_key: stringOrNull(input.effectiveConfig.provider_key),
      model_key: stringOrNull(input.effectiveConfig.model_id),
      effective_config_json: input.effectiveConfig
    });
  }

  heartbeat(runId: string): Promise<{ run: ActionRunView }> {
    return this.post(`/api/v1/review-agent/action/runs/${runId}/heartbeat`, {});
  }

  completeRun(runId: string, metrics: RunMetrics): Promise<{ run: ActionRunView }> {
    return this.post(`/api/v1/review-agent/action/runs/${runId}/complete`, metrics);
  }

  failRun(runId: string, errorSummary: string, metrics: RunMetrics): Promise<{ run: ActionRunView }> {
    return this.post(`/api/v1/review-agent/action/runs/${runId}/fail`, {
      error_summary: errorSummary,
      metrics
    });
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method: "POST",
      headers: {
        authorization: `Bearer ${this.apiKey}`,
        "content-type": "application/json"
      },
      signal: AbortSignal.timeout(this.timeoutMs),
      body: JSON.stringify(body)
    });

    const text = await response.text();
    const parsed = text ? JSON.parse(text) : {};
    if (!response.ok) {
      const detail = parsed?.error?.message ?? parsed?.message ?? response.statusText;
      throw new Error(`Oceans API ${response.status}: ${detail}`);
    }
    return (parsed as Envelope<T>).data;
  }
}

function stringOrNull(value: unknown): string | undefined {
  return typeof value === "string" && value ? value : undefined;
}
