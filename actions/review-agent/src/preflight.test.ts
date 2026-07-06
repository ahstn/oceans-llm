import { describe, expect, test } from "bun:test";
import { validateCheckoutHead, validatePullRequestPreflight } from "./preflight";
import type { PullRequestContext } from "./types";

const baseContext: PullRequestContext = {
  repository: {
    provider: "github",
    owner: "octo",
    name: "repo",
    full_name: "octo/repo"
  },
  pullRequest: {
    pr_number: 42,
    head_sha: "abc",
    base_sha: "def",
    head_repository_full_name: "octo/repo",
    base_repository_full_name: "octo/repo",
    is_draft: false
  }
};

describe("preflight", () => {
  test("accepts same-repo pull requests", () => {
    expect(validatePullRequestPreflight("pull_request", baseContext)).toEqual({ ok: true });
  });

  test("skips drafts and forks", () => {
    expect(validatePullRequestPreflight("pull_request", {
      ...baseContext,
      pullRequest: { ...baseContext.pullRequest, is_draft: true }
    })).toEqual({ ok: false, reason: "Draft pull request skipped." });

    const fork = validatePullRequestPreflight("pull_request", {
      ...baseContext,
      pullRequest: { ...baseContext.pullRequest, head_repository_full_name: "someone/repo" }
    });
    expect(fork.ok).toBe(false);
  });

  test("validates checked-out head", () => {
    expect(validateCheckoutHead("abc", "/tmp", () => "abc\n")).toEqual({ ok: true });
    expect(validateCheckoutHead("abc", "/tmp", () => "def\n").ok).toBe(false);
  });
});
