import { readFileSync } from "node:fs";
import type { PullRequestContext } from "./types";

export interface GitHubRuntimeContext {
  eventName: string;
  eventPayload: Record<string, any>;
  repository: string;
  runId?: string;
  runAttempt?: number;
  workspace: string;
}

export function readGitHubContext(env: NodeJS.ProcessEnv): GitHubRuntimeContext {
  const eventPath = env.GITHUB_EVENT_PATH;
  if (!eventPath) {
    throw new Error("GITHUB_EVENT_PATH is required");
  }
  return {
    eventName: env.GITHUB_EVENT_NAME ?? "",
    eventPayload: JSON.parse(readFileSync(eventPath, "utf8")),
    repository: env.GITHUB_REPOSITORY ?? "",
    runId: env.GITHUB_RUN_ID,
    runAttempt: env.GITHUB_RUN_ATTEMPT ? Number(env.GITHUB_RUN_ATTEMPT) : undefined,
    workspace: env.GITHUB_WORKSPACE ?? process.cwd()
  };
}

export function extractPullRequestContext(payload: Record<string, any>, fallbackRepository: string): PullRequestContext {
  const pull = payload.pull_request;
  if (!pull) {
    throw new Error("pull_request payload is required");
  }
  const repo = payload.repository ?? pull.base?.repo;
  const fullName = repo?.full_name ?? fallbackRepository;
  const [owner, name] = fullName.split("/");
  if (!owner || !name) {
    throw new Error("GITHUB_REPOSITORY must be in owner/name form");
  }

  return {
    repository: {
      provider: "github",
      external_repository_id: repo?.id === undefined ? undefined : String(repo.id),
      owner,
      name,
      full_name: fullName
    },
    pullRequest: {
      provider_pr_id: pull.id === undefined ? undefined : String(pull.id),
      pr_number: Number(pull.number),
      title: pull.title ?? undefined,
      author_login: pull.user?.login ?? undefined,
      head_sha: pull.head?.sha ?? undefined,
      base_sha: pull.base?.sha ?? undefined,
      head_repository_full_name: pull.head?.repo?.full_name ?? "",
      base_repository_full_name: pull.base?.repo?.full_name ?? fullName,
      is_draft: Boolean(pull.draft)
    }
  };
}
