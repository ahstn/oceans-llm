import * as core from "@actions/core";
import * as github from "@actions/github";
import { buildOverrides, envInputReader, parseInputs } from "./input";
import { extractPullRequestContext, readGitHubContext } from "./github-context";
import { validateCheckoutHead, validatePullRequestPreflight } from "./preflight";
import { OceansClient } from "./oceans-client";
import { cleanupPiInvocation, invokePi, preparePiInvocation, resolvePiBinary } from "./pi";
import { Redactor } from "./redaction";
import { buildJobSummary, GitHubPublisher } from "./summary";
import type { EffectiveConfig, ReviewResult, RunMetrics } from "./types";

export async function run(): Promise<void> {
  const inputs = parseInputs((name) => core.getInput(name));
  for (const secret of [inputs.oceansApiKey, inputs.githubToken, inputs.providerKey]) {
    if (secret) core.setSecret(secret);
  }
  const redactor = new Redactor([inputs.oceansApiKey, inputs.githubToken, inputs.providerKey]);
  const runtime = readGitHubContext(process.env);

  if (runtime.eventName !== "pull_request") {
    const reason = "Review agent only runs on pull_request events.";
    core.warning(reason);
    await core.summary.addRaw(`## Oceans LLM Review Agent\n\n${reason}`).write();
    return;
  }

  const pullRequestContext = extractPullRequestContext(runtime.eventPayload, runtime.repository);
  const eventCheck = validatePullRequestPreflight(runtime.eventName, pullRequestContext);
  if (!eventCheck.ok) {
    core.warning(eventCheck.reason);
    await core.summary.addRaw(`## Oceans LLM Review Agent\n\n${eventCheck.reason}`).write();
    return;
  }
  const checkoutCheck = validateCheckoutHead(pullRequestContext.pullRequest.head_sha!, runtime.workspace);
  if (!checkoutCheck.ok) {
    core.warning(checkoutCheck.reason);
    await core.summary.addRaw(`## Oceans LLM Review Agent\n\n${checkoutCheck.reason}`).write();
    return;
  }
  if (!inputs.githubToken) {
    throw new Error("github-token or GITHUB_TOKEN is required");
  }

  const client = new OceansClient(inputs.oceansUrl, inputs.oceansApiKey);
  const resolved = await client.resolveConfig({
    eventName: runtime.eventName,
    repository: pullRequestContext.repository,
    pullRequest: pullRequestContext.pullRequest,
    overrides: buildOverrides(inputs)
  });
  const config = resolved.effective_config;
  if (!hasEffectiveModel(config)) {
    core.warning("No effective review model is configured; skipping review agent run.");
    await core.summary.addRaw("## Oceans LLM Review Agent\n\nNo effective review model is configured.").write();
    return;
  }

  const started = await client.startRun({
    eventName: runtime.eventName,
    repository: pullRequestContext.repository,
    pullRequest: pullRequestContext.pullRequest,
    githubRunId: runtime.runId,
    githubRunAttempt: runtime.runAttempt,
    effectiveConfig: config
  });

  let result: ReviewResult | undefined;
  try {
    const piBinary = resolvePiBinary(inputs.piBinary);
    const invocation = preparePiInvocation({ piBinary, context: pullRequestContext, effectiveConfig: config });
    try {
      result = invokePi(invocation, inputs.timeoutMinutes);
    } finally {
      cleanupPiInvocation(invocation);
    }
    const publisher = new GitHubPublisher(github.getOctokit(inputs.githubToken));
    const [owner, repo] = pullRequestContext.repository.full_name.split("/");
    const publishMetrics = await publisher.publish({
      owner,
      repo,
      prNumber: pullRequestContext.pullRequest.pr_number,
      headSha: pullRequestContext.pullRequest.head_sha!,
      result,
      inlineReview: config.inline_review_enabled ?? true,
      prSummary: config.pr_summary_enabled ?? true,
      maxInlineComments: config.max_inline_comments ?? 20,
      requestChangesOnHighSeverity: config.request_changes_on_high_severity ?? false,
      dryRun: inputs.dryRun
    });
    const metrics = completeMetrics(result, publishMetrics);
    await client.completeRun(started.run.id, metrics);
    await core.summary.addRaw(buildJobSummary(result, result.degradedFeatures)).write();
  } catch (error) {
    const metrics = result ? completeMetrics(result, {}) : { status: "failed" as const };
    await client.failRun(started.run.id, redactor.errorSummary(error), metrics).catch((failError) => {
      core.warning(redactor.errorSummary(failError));
    });
    throw error;
  }
}

function hasEffectiveModel(config: EffectiveConfig): boolean {
  return typeof config.model_id === "string" && config.model_id.length > 0;
}

function completeMetrics(result: ReviewResult, publishMetrics: RunMetrics): RunMetrics {
  return {
    ...result.metrics,
    ...publishMetrics,
    status: "succeeded",
    degraded_features_json: result.degradedFeatures.length > 0 ? result.degradedFeatures : undefined
  };
}
