import { describe, expect, test } from "bun:test";
import { buildManagedComment, GitHubPublisher } from "./summary";
import type { ReviewResult } from "./types";

const result: ReviewResult = {
  summary: "Summary text",
  findings: [{ path: "src/lib.ts", line: 5, message: "Fix it", severity: "high" }],
  metrics: {},
  degradedFeatures: []
};

describe("summary publishing", () => {
  test("builds a managed comment with marker", () => {
    const body = buildManagedComment(result);
    expect(body).toContain("<!-- oceans-llm-review-agent -->");
    expect(body).toContain("High severity findings: 1");
  });

  test("updates an existing managed comment", async () => {
    const calls: string[] = [];
    const publisher = new GitHubPublisher({
      paginate: async () => [{ id: 99, body: "old\n<!-- oceans-llm-review-agent -->" }],
      rest: {
        issues: {
          listComments: {},
          updateComment: async () => calls.push("update"),
          createComment: async () => {
            calls.push("create");
            return { data: { id: 100 } };
          }
        },
        pulls: {
          createReview: async () => calls.push("review")
        }
      }
    });

    const metrics = await publisher.publish({
      owner: "octo",
      repo: "repo",
      prNumber: 1,
      headSha: "abc",
      result,
      inlineReview: false,
      prSummary: true,
      maxInlineComments: 10,
      requestChangesOnHighSeverity: false,
      dryRun: false
    });

    expect(calls).toEqual(["update"]);
    expect(metrics.managed_comment_id).toBe("99");
    expect(metrics.managed_comment_action).toBe("updated");
  });
});
