import { readFileSync } from "node:fs";
import type { Finding, ReviewResult, RunMetrics } from "./types";

export function readReviewResult(path: string): ReviewResult {
  const parsed = JSON.parse(readFileSync(path, "utf8"));
  const findings = Array.isArray(parsed.findings) ? parsed.findings.map(sanitizeFinding).filter(Boolean) : [];
  const metrics = sanitizeMetrics(parsed.metrics ?? {});
  return {
    summary: typeof parsed.summary === "string" ? parsed.summary.slice(0, 60000) : undefined,
    findings,
    metrics: {
      ...metrics,
      files_changed: metrics.files_changed ?? countUniqueFiles(findings),
      inline_comments_skipped: metrics.inline_comments_skipped ?? 0
    },
    degradedFeatures: Array.isArray(parsed.degraded_features)
      ? parsed.degraded_features.filter((item: unknown) => typeof item === "string").slice(0, 20)
      : []
  };
}

export function sanitizeMetrics(input: Record<string, unknown>): RunMetrics {
  return {
    duration_ms: nonNegative(input.duration_ms),
    files_changed: nonNegative(input.files_changed),
    additions: nonNegative(input.additions),
    deletions: nonNegative(input.deletions),
    changed_loc: nonNegative(input.changed_loc),
    inline_comments_created: nonNegative(input.inline_comments_created),
    inline_comments_updated: nonNegative(input.inline_comments_updated),
    inline_comments_skipped: nonNegative(input.inline_comments_skipped),
    inline_comments_failed: nonNegative(input.inline_comments_failed),
    stale_comments_deleted: nonNegative(input.stale_comments_deleted),
    managed_comment_id: shortString(input.managed_comment_id),
    managed_comment_action: shortString(input.managed_comment_action),
    managed_comment_status: shortString(input.managed_comment_status),
    review_event_status: shortString(input.review_event_status),
    summary_status: shortString(input.summary_status),
    diagram_status: shortString(input.diagram_status),
    linked_issue_count: nonNegative(input.linked_issue_count),
    linked_issue_status: shortString(input.linked_issue_status)
  };
}

function sanitizeFinding(input: any): Finding | undefined {
  if (typeof input?.path !== "string" || !Number.isInteger(input?.line) || typeof input?.message !== "string") {
    return undefined;
  }
  return {
    path: input.path,
    line: input.line,
    start_line: Number.isInteger(input.start_line) ? input.start_line : undefined,
    end_line: Number.isInteger(input.end_line) ? input.end_line : undefined,
    side: input.side === "LEFT" ? "LEFT" : "RIGHT",
    severity: typeof input.severity === "string" ? input.severity : undefined,
    message: input.message.slice(0, 12000)
  };
}

function nonNegative(value: unknown): number | undefined {
  return Number.isInteger(value) && Number(value) >= 0 ? Number(value) : undefined;
}

function shortString(value: unknown): string | undefined {
  return typeof value === "string" ? value.slice(0, 100) : undefined;
}

function countUniqueFiles(findings: Finding[]): number {
  return new Set(findings.map((finding) => finding.path)).size;
}
