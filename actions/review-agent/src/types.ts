export type BoolInput = boolean | undefined;

export interface ActionInputs {
  oceansUrl: string;
  oceansApiKey: string;
  modelId?: string;
  modelMode?: string;
  providerKey?: string;
  inlineReview?: BoolInput;
  prSummary?: BoolInput;
  diagrams?: BoolInput;
  linkedIssueDetection?: BoolInput;
  linkedIssueAssessment?: BoolInput;
  timeoutMinutes: number;
  maxInlineComments?: number;
  requestChangesOnHighSeverity?: BoolInput;
  dryRun: boolean;
  debug: boolean;
  piBinary?: string;
  githubToken?: string;
}

export interface RepositoryIdentity {
  provider: "github";
  external_repository_id?: string;
  owner: string;
  name: string;
  full_name: string;
}

export interface PullRequestIdentity {
  provider_pr_id?: string;
  pr_number: number;
  title?: string;
  author_login?: string;
  head_sha?: string;
  base_sha?: string;
  head_repository_full_name: string;
  base_repository_full_name: string;
  is_draft: boolean;
}

export interface PullRequestContext {
  repository: RepositoryIdentity;
  pullRequest: PullRequestIdentity;
}

export interface EffectiveConfig {
  model_id?: string | null;
  model_execution_mode?: "oceans" | "direct" | string | null;
  provider_key?: string | null;
  inline_review_enabled?: boolean;
  pr_summary_enabled?: boolean;
  diagrams_enabled?: boolean;
  linked_issue_detection_enabled?: boolean;
  linked_issue_assessment_enabled?: boolean;
  max_inline_comments?: number | null;
  request_changes_on_high_severity?: boolean;
  oceans_base_url?: string | null;
  [key: string]: unknown;
}

export interface RunMetrics {
  status?: "succeeded" | "failed" | "cancelled" | "skipped";
  duration_ms?: number;
  files_changed?: number;
  additions?: number;
  deletions?: number;
  changed_loc?: number;
  inline_comments_created?: number;
  inline_comments_updated?: number;
  inline_comments_skipped?: number;
  inline_comments_failed?: number;
  stale_comments_deleted?: number;
  managed_comment_id?: string;
  managed_comment_action?: string;
  managed_comment_status?: string;
  review_event_status?: string;
  summary_status?: string;
  diagram_status?: string;
  linked_issue_count?: number;
  linked_issue_status?: string;
  degraded_features_json?: unknown;
}

export interface Finding {
  path: string;
  line: number;
  start_line?: number;
  end_line?: number;
  side?: "RIGHT" | "LEFT";
  severity?: "low" | "medium" | "high" | "critical" | string;
  message: string;
}

export interface ReviewResult {
  summary?: string;
  findings: Finding[];
  metrics: RunMetrics;
  degradedFeatures: string[];
}
