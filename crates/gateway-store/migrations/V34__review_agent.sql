CREATE TABLE IF NOT EXISTS review_agent_repositories (
  repository_id TEXT PRIMARY KEY,
  provider TEXT NOT NULL CHECK (provider IN ('github')),
  external_repository_id TEXT,
  owner TEXT NOT NULL,
  name TEXT NOT NULL,
  full_name TEXT NOT NULL,
  service_account_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled', 'archived')),
  inline_review_enabled INTEGER NOT NULL DEFAULT 1 CHECK (inline_review_enabled IN (0, 1)),
  pr_summary_enabled INTEGER NOT NULL DEFAULT 1 CHECK (pr_summary_enabled IN (0, 1)),
  diagrams_enabled INTEGER NOT NULL DEFAULT 0 CHECK (diagrams_enabled IN (0, 1)),
  linked_issue_detection_enabled INTEGER NOT NULL DEFAULT 1 CHECK (linked_issue_detection_enabled IN (0, 1)),
  linked_issue_assessment_enabled INTEGER NOT NULL DEFAULT 0 CHECK (linked_issue_assessment_enabled IN (0, 1)),
  default_model_key TEXT,
  max_inline_comments INTEGER CHECK (max_inline_comments IS NULL OR max_inline_comments >= 0),
  request_changes_on_high_severity INTEGER NOT NULL DEFAULT 0 CHECK (request_changes_on_high_severity IN (0, 1)),
  settings_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS review_agent_repositories_external_id_uidx
  ON review_agent_repositories (provider, external_repository_id)
  WHERE external_repository_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS review_agent_repositories_active_owner_name_uidx
  ON review_agent_repositories (provider, lower(owner), lower(name))
  WHERE status = 'active';

CREATE INDEX IF NOT EXISTS review_agent_repositories_service_account_idx
  ON review_agent_repositories (service_account_id);

CREATE INDEX IF NOT EXISTS review_agent_repositories_status_idx
  ON review_agent_repositories (status);

CREATE TABLE IF NOT EXISTS review_agent_pull_requests (
  pull_request_id TEXT PRIMARY KEY,
  repository_id TEXT NOT NULL,
  provider_pr_id TEXT,
  pr_number INTEGER NOT NULL CHECK (pr_number > 0),
  title TEXT,
  author_login TEXT,
  state TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged', 'unknown')),
  head_sha TEXT,
  base_sha TEXT,
  head_repository_full_name TEXT,
  base_repository_full_name TEXT,
  is_draft INTEGER NOT NULL DEFAULT 0 CHECK (is_draft IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (repository_id) REFERENCES review_agent_repositories(repository_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS review_agent_pull_requests_repo_number_uidx
  ON review_agent_pull_requests (repository_id, pr_number);

CREATE INDEX IF NOT EXISTS review_agent_pull_requests_repo_state_idx
  ON review_agent_pull_requests (repository_id, state);

CREATE TABLE IF NOT EXISTS review_agent_runs (
  run_id TEXT PRIMARY KEY,
  repository_id TEXT NOT NULL,
  pull_request_id TEXT,
  head_sha TEXT,
  github_run_id TEXT,
  github_run_attempt INTEGER CHECK (github_run_attempt IS NULL OR github_run_attempt > 0),
  status TEXT NOT NULL CHECK (status IN ('queued', 'in_progress', 'succeeded', 'failed', 'cancelled', 'skipped')),
  started_at INTEGER,
  heartbeat_at INTEGER,
  finished_at INTEGER,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  files_changed INTEGER CHECK (files_changed IS NULL OR files_changed >= 0),
  additions INTEGER CHECK (additions IS NULL OR additions >= 0),
  deletions INTEGER CHECK (deletions IS NULL OR deletions >= 0),
  changed_loc INTEGER CHECK (changed_loc IS NULL OR changed_loc >= 0),
  inline_comments_created INTEGER CHECK (inline_comments_created IS NULL OR inline_comments_created >= 0),
  inline_comments_updated INTEGER CHECK (inline_comments_updated IS NULL OR inline_comments_updated >= 0),
  inline_comments_skipped INTEGER CHECK (inline_comments_skipped IS NULL OR inline_comments_skipped >= 0),
  inline_comments_failed INTEGER CHECK (inline_comments_failed IS NULL OR inline_comments_failed >= 0),
  stale_comments_deleted INTEGER CHECK (stale_comments_deleted IS NULL OR stale_comments_deleted >= 0),
  managed_comment_id TEXT,
  managed_comment_action TEXT,
  managed_comment_status TEXT,
  review_event_status TEXT,
  summary_status TEXT,
  diagram_status TEXT,
  linked_issue_count INTEGER CHECK (linked_issue_count IS NULL OR linked_issue_count >= 0),
  linked_issue_status TEXT,
  model_execution_mode TEXT,
  provider_key TEXT,
  model_key TEXT,
  effective_config_json TEXT NOT NULL,
  degraded_features_json TEXT,
  error_summary TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (repository_id) REFERENCES review_agent_repositories(repository_id) ON DELETE CASCADE,
  FOREIGN KEY (pull_request_id) REFERENCES review_agent_pull_requests(pull_request_id) ON DELETE SET NULL,
  CHECK (finished_at IS NULL OR started_at IS NULL OR finished_at >= started_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS review_agent_runs_github_attempt_uidx
  ON review_agent_runs (repository_id, github_run_id, github_run_attempt)
  WHERE github_run_id IS NOT NULL AND github_run_attempt IS NOT NULL;

CREATE INDEX IF NOT EXISTS review_agent_runs_repository_created_idx
  ON review_agent_runs (repository_id, created_at DESC);

CREATE INDEX IF NOT EXISTS review_agent_runs_pull_request_created_idx
  ON review_agent_runs (pull_request_id, created_at DESC);
