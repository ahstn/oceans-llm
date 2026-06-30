import { describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readReviewResult, sanitizeMetrics } from "./result-artifact";

describe("result artifact", () => {
  test("drops invalid findings and non-metric fields", () => {
    const tempDir = mkdtempSync(join(tmpdir(), "oceans-result-"));
    const path = join(tempDir, "result.json");
    writeFileSync(path, JSON.stringify({
      summary: "review summary",
      raw_diff: "must not be reported",
      findings: [
        { path: "src/lib.ts", line: 9, message: "Fix this", severity: "high" },
        { path: "missing-message.ts", line: 10 }
      ],
      metrics: {
        files_changed: 5,
        inline_comments_created: -1,
        managed_comment_status: "ok"
      },
      degraded_features: ["linked_issues"]
    }));

    const result = readReviewResult(path);
    expect(result.findings).toHaveLength(1);
    expect(result.metrics.files_changed).toBe(5);
    expect(result.metrics.inline_comments_created).toBeUndefined();
    expect(result.degradedFeatures).toEqual(["linked_issues"]);
  });

  test("sanitizes only numeric and short status metrics", () => {
    expect(sanitizeMetrics({
      duration_ms: 100,
      prompt: "do not keep",
      summary_status: "succeeded"
    })).toEqual({
      duration_ms: 100,
      files_changed: undefined,
      additions: undefined,
      deletions: undefined,
      changed_loc: undefined,
      inline_comments_created: undefined,
      inline_comments_updated: undefined,
      inline_comments_skipped: undefined,
      inline_comments_failed: undefined,
      stale_comments_deleted: undefined,
      managed_comment_id: undefined,
      managed_comment_action: undefined,
      managed_comment_status: undefined,
      review_event_status: undefined,
      summary_status: "succeeded",
      diagram_status: undefined,
      linked_issue_count: undefined,
      linked_issue_status: undefined
    });
  });
});
