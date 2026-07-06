import type { ActionInputs } from "./types";

type InputReader = (name: string) => string;

export function parseInputs(readInput: InputReader): ActionInputs {
  const oceansUrl = required(readInput, "oceans-url");
  const oceansApiKey = required(readInput, "oceans-api-key");
  const timeoutMinutes = parsePositiveInteger(readInput("timeout-minutes") || "20", "timeout-minutes");

  return {
    oceansUrl: normalizeUrl(oceansUrl),
    oceansApiKey,
    modelId: optional(readInput("model-id")),
    modelMode: optional(readInput("model-mode")),
    providerKey: optional(readInput("provider-key")),
    inlineReview: parseOptionalBoolean(readInput("inline-review"), "inline-review"),
    prSummary: parseOptionalBoolean(readInput("pr-summary"), "pr-summary"),
    diagrams: parseOptionalBoolean(readInput("diagrams"), "diagrams"),
    linkedIssueDetection: parseOptionalBoolean(readInput("linked-issue-detection"), "linked-issue-detection"),
    linkedIssueAssessment: parseOptionalBoolean(readInput("linked-issue-assessment"), "linked-issue-assessment"),
    timeoutMinutes,
    maxInlineComments: parseOptionalInteger(readInput("max-inline-comments"), "max-inline-comments"),
    requestChangesOnHighSeverity: parseOptionalBoolean(
      readInput("request-changes-on-high-severity"),
      "request-changes-on-high-severity"
    ),
    dryRun: parseBoolean(readInput("dry-run") || "false", "dry-run"),
    debug: parseBoolean(readInput("debug") || "false", "debug"),
    piBinary: optional(readInput("pi-binary")),
    githubToken: optional(readInput("github-token")) || optional(process.env.GITHUB_TOKEN)
  };
}

export function envInputReader(env: NodeJS.ProcessEnv): InputReader {
  return (name: string) => env[`INPUT_${name.replace(/ /g, "_").toUpperCase()}`] ?? "";
}

export function buildOverrides(inputs: ActionInputs): Record<string, unknown> {
  return removeUndefined({
    model_id: inputs.modelId,
    model_execution_mode: inputs.modelMode,
    provider_key: inputs.providerKey,
    inline_review_enabled: inputs.inlineReview,
    pr_summary_enabled: inputs.prSummary,
    diagrams_enabled: inputs.diagrams,
    linked_issue_detection_enabled: inputs.linkedIssueDetection,
    linked_issue_assessment_enabled: inputs.linkedIssueAssessment,
    max_inline_comments: inputs.maxInlineComments,
    request_changes_on_high_severity: inputs.requestChangesOnHighSeverity
  });
}

function required(readInput: InputReader, name: string): string {
  const value = optional(readInput(name));
  if (!value) {
    throw new Error(`Missing required input: ${name}`);
  }
  return value;
}

function optional(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

function normalizeUrl(value: string): string {
  const parsed = new URL(value);
  return parsed.toString().replace(/\/$/, "");
}

function parseBoolean(value: string, name: string): boolean {
  const normalized = value.trim().toLowerCase();
  if (["true", "1", "yes", "on"].includes(normalized)) return true;
  if (["false", "0", "no", "off"].includes(normalized)) return false;
  throw new Error(`Input ${name} must be a boolean`);
}

function parseOptionalBoolean(value: string, name: string): boolean | undefined {
  return optional(value) === undefined ? undefined : parseBoolean(value, name);
}

function parsePositiveInteger(value: string, name: string): number {
  const parsed = parseOptionalInteger(value, name);
  if (parsed === undefined || parsed <= 0) {
    throw new Error(`Input ${name} must be a positive integer`);
  }
  return parsed;
}

function parseOptionalInteger(value: string, name: string): number | undefined {
  if (optional(value) === undefined) return undefined;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`Input ${name} must be a non-negative integer`);
  }
  return parsed;
}

function removeUndefined(input: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(input).filter(([, value]) => value !== undefined));
}
