import type { Finding, ReviewResult, RunMetrics } from "./types";

const MARKER = "<!-- oceans-llm-review-agent -->";

export interface Publisher {
  publish(input: PublishInput): Promise<RunMetrics>;
}

export interface PublishInput {
  owner: string;
  repo: string;
  prNumber: number;
  headSha: string;
  result: ReviewResult;
  inlineReview: boolean;
  prSummary: boolean;
  maxInlineComments: number;
  requestChangesOnHighSeverity: boolean;
  dryRun: boolean;
}

export function buildJobSummary(result: ReviewResult, degradedFeatures: string[]): string {
  const high = result.findings.filter((finding) => isHighSeverity(finding)).length;
  const lines = [
    "## Oceans LLM Review Agent",
    "",
    `Findings: ${result.findings.length}`,
    `High severity findings: ${high}`
  ];
  if (degradedFeatures.length > 0) {
    lines.push(`Degraded features: ${degradedFeatures.join(", ")}`);
  }
  return lines.join("\n");
}

export function buildManagedComment(result: ReviewResult): string {
  const high = result.findings.filter((finding) => isHighSeverity(finding)).length;
  const summary = result.summary ? `\n\n${result.summary}` : "";
  return `${MARKER}\n## Oceans LLM Review\n\nFindings: ${result.findings.length}\nHigh severity findings: ${high}${summary}`;
}

export function isHighSeverity(finding: Finding): boolean {
  return finding.severity === "high" || finding.severity === "critical";
}

export class GitHubPublisher implements Publisher {
  constructor(private readonly octokit: any) {}

  async publish(input: PublishInput): Promise<RunMetrics> {
    if (input.dryRun) {
      return {
        inline_comments_skipped: Math.min(input.result.findings.length, input.maxInlineComments),
        managed_comment_status: "dry_run",
        review_event_status: "dry_run",
        summary_status: "dry_run"
      };
    }

    const metrics: RunMetrics = {};
    if (input.prSummary) {
      const comment = await upsertManagedComment(this.octokit, input);
      metrics.managed_comment_id = String(comment.id);
      metrics.managed_comment_action = comment.action;
      metrics.managed_comment_status = "succeeded";
      metrics.summary_status = "succeeded";
    }

    if (input.inlineReview) {
      const comments = input.result.findings.slice(0, input.maxInlineComments).map((finding) => ({
        path: finding.path,
        line: finding.line,
        side: finding.side ?? "RIGHT",
        body: `[${finding.severity ?? "medium"}] ${finding.message}`
      }));
      const event =
        input.requestChangesOnHighSeverity && input.result.findings.some(isHighSeverity) ? "REQUEST_CHANGES" : "COMMENT";
      if (comments.length > 0 || event === "REQUEST_CHANGES") {
        await this.octokit.rest.pulls.createReview({
          owner: input.owner,
          repo: input.repo,
          pull_number: input.prNumber,
          commit_id: input.headSha,
          event,
          body: buildManagedComment(input.result),
          comments
        });
      }
      metrics.inline_comments_created = comments.length;
      metrics.inline_comments_skipped = Math.max(0, input.result.findings.length - comments.length);
      metrics.review_event_status = "succeeded";
    }
    return metrics;
  }
}

async function upsertManagedComment(octokit: any, input: PublishInput): Promise<{ id: number; action: string }> {
  const body = buildManagedComment(input.result);
  const existing = await findManagedComment(octokit, input);
  if (existing) {
    await octokit.rest.issues.updateComment({
      owner: input.owner,
      repo: input.repo,
      comment_id: existing.id,
      body
    });
    return { id: existing.id, action: "updated" };
  }
  const created = await octokit.rest.issues.createComment({
    owner: input.owner,
    repo: input.repo,
    issue_number: input.prNumber,
    body
  });
  return { id: created.data.id, action: "created" };
}

async function findManagedComment(octokit: any, input: PublishInput): Promise<any | undefined> {
  for (let page = 1; ; page += 1) {
    const response = await octokit.rest.issues.listComments({
      owner: input.owner,
      repo: input.repo,
      issue_number: input.prNumber,
      per_page: 100,
      page
    });
    const existing = response.data.find(
      (comment: any) => typeof comment.body === "string" && comment.body.includes(MARKER)
    );
    if (existing || response.data.length < 100) {
      return existing;
    }
  }
}
