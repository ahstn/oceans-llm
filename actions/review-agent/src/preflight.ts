import { execFileSync } from "node:child_process";
import type { PullRequestContext } from "./types";

export interface PreflightSkip {
  ok: false;
  reason: string;
}

export interface PreflightOk {
  ok: true;
}

export type PreflightResult = PreflightOk | PreflightSkip;

export type GitRunner = (args: string[], cwd: string) => string;

export function validatePullRequestPreflight(eventName: string, context: PullRequestContext): PreflightResult {
  if (eventName !== "pull_request") {
    return { ok: false, reason: "Review agent only runs on pull_request events." };
  }
  if (context.pullRequest.is_draft) {
    return { ok: false, reason: "Draft pull request skipped." };
  }
  if (context.pullRequest.head_repository_full_name !== context.pullRequest.base_repository_full_name) {
    return { ok: false, reason: "Fork pull request skipped; only same-repository PRs are supported." };
  }
  if (!context.pullRequest.head_sha) {
    return { ok: false, reason: "Pull request head SHA is missing." };
  }
  return { ok: true };
}

export function validateCheckoutHead(expectedSha: string, cwd: string, git: GitRunner = defaultGit): PreflightResult {
  const actualSha = git(["rev-parse", "HEAD"], cwd).trim();
  if (actualSha !== expectedSha) {
    return { ok: false, reason: `Checked-out HEAD ${actualSha} does not match PR head ${expectedSha}.` };
  }
  return { ok: true };
}

function defaultGit(args: string[], cwd: string): string {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}
