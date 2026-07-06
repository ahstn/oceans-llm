import { describe, expect, test } from "bun:test";
import { OceansClient } from "./oceans-client";

describe("OceansClient", () => {
  test("posts resolve config requests with bearer auth", async () => {
    const requests: Array<{ url: string; body: any; authorization: string | null; signal?: AbortSignal | null }> = [];
    const client = new OceansClient("https://oceans.example.test", "secret", (async (url, init) => {
      requests.push({
        url: String(url),
        body: JSON.parse(String(init?.body)),
        authorization: new Headers(init?.headers).get("authorization"),
        signal: init?.signal
      });
      return new Response(JSON.stringify({
        data: {
          repository: {},
          pull_request_id: "pr",
          effective_config: { model_id: "gpt-5" },
          overrides_applied: {},
          overrides_rejected: {},
          reporting: {}
        }
      }), { status: 200 });
    }) as typeof fetch);

    await client.resolveConfig({
      eventName: "pull_request",
      repository: { provider: "github", owner: "octo", name: "repo", full_name: "octo/repo" },
      pullRequest: {
        pr_number: 1,
        head_repository_full_name: "octo/repo",
        base_repository_full_name: "octo/repo",
        is_draft: false
      },
      overrides: { inline_review_enabled: false }
    });

    expect(requests[0].url).toBe("https://oceans.example.test/api/v1/review-agent/action/config/resolve");
    expect(requests[0].authorization).toBe("Bearer secret");
    expect(requests[0].signal).toBeInstanceOf(AbortSignal);
    expect(requests[0].body.overrides).toEqual({ inline_review_enabled: false });
  });
});
