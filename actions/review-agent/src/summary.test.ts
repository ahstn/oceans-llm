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
      rest: {
        issues: {
          listComments: async () => ({
            data: [{ id: 99, body: "old\n<!-- oceans-llm-review-agent -->" }]
          }),
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

  test("requests changes even when no inline comments are emitted", async () => {
    const reviews: any[] = [];
    const publisher = new GitHubPublisher({
      rest: {
        issues: {
          listComments: async () => ({ data: [] }),
          updateComment: async () => undefined,
          createComment: async () => ({ data: { id: 100 } })
        },
        pulls: {
          createReview: async (input: any) => reviews.push(input)
        }
      }
    });

    await publisher.publish({
      owner: "octo",
      repo: "repo",
      prNumber: 1,
      headSha: "abc",
      result,
      inlineReview: true,
      prSummary: false,
      maxInlineComments: 0,
      requestChangesOnHighSeverity: true,
      dryRun: false
    });

    expect(reviews).toHaveLength(1);
    expect(reviews[0].event).toBe("REQUEST_CHANGES");
    expect(reviews[0].comments).toEqual([]);
    expect(reviews[0].body).toContain("High severity findings: 1");
  });
});
