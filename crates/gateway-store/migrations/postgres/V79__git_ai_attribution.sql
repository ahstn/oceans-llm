SET LOCAL lock_timeout = '5s';
-- Longer than other migrations' 60s: the agent_session_requests index at the end
-- reads every existing request row.
SET LOCAL statement_timeout = '15min';

-- Git AI attribution (docs/adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md).
-- Rows belong to the ownership scope of the API key that reported them. Agent
-- sessions are stored only as scope-bound lineage hashes, and authors as an
-- Oceans user id or a scope-bound email hash, never as raw ids or emails.
-- Times are unix milliseconds.

-- One row per reported note, keyed by (ownership_scope_key, repository,
-- commit_sha). A newer note replaces the row in place, keeping its id and
-- `first_observed_at`, the commit time used for request windows.
CREATE TABLE IF NOT EXISTS attribution_commits (
  attribution_commit_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_user_id TEXT,
  owner_service_account_id TEXT,
  repository TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  branch TEXT,
  is_default_branch BOOLEAN NOT NULL,
  author_user_id TEXT,
  author_email_hash TEXT,
  schema_version TEXT NOT NULL,
  git_ai_version TEXT,
  ai_lines BIGINT NOT NULL CHECK (ai_lines >= 0),
  human_lines BIGINT NOT NULL CHECK (human_lines >= 0),
  installation_id TEXT NOT NULL,
  sequence BIGINT NOT NULL,
  observed_at BIGINT NOT NULL,
  late_sequenced BOOLEAN NOT NULL,
  first_observed_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (ownership_scope_key, repository, commit_sha),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (author_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_attribution_commits_observed
  ON attribution_commits(first_observed_at);
CREATE INDEX IF NOT EXISTS idx_attribution_commits_owner_user_observed
  ON attribution_commits(owner_user_id, first_observed_at);
CREATE INDEX IF NOT EXISTS idx_attribution_commits_author_user_observed
  ON attribution_commits(author_user_id, first_observed_at);

-- The sessions a commit's note mentions, replaced together with the commit row.
CREATE TABLE IF NOT EXISTS attribution_commit_sessions (
  attribution_commit_id TEXT NOT NULL,
  note_session_key TEXT NOT NULL,
  agent_tool TEXT NOT NULL,
  agent_model TEXT,
  harness_key TEXT,
  ai_lines BIGINT NOT NULL CHECK (ai_lines >= 0),
  human_author_user_id TEXT,
  human_author_hash TEXT,
  PRIMARY KEY (attribution_commit_id, note_session_key),
  FOREIGN KEY (attribution_commit_id) REFERENCES attribution_commits(attribution_commit_id) ON DELETE CASCADE,
  FOREIGN KEY (human_author_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

-- A commit session's id hashed under every session namespace the gateway
-- records requests under, replaced together with the session rows. The
-- session joins the requests of whichever hash the gateway used.
CREATE TABLE IF NOT EXISTS attribution_commit_session_hashes (
  attribution_commit_id TEXT NOT NULL,
  note_session_key TEXT NOT NULL,
  session_hash TEXT NOT NULL,
  PRIMARY KEY (attribution_commit_id, note_session_key, session_hash),
  FOREIGN KEY (attribution_commit_id, note_session_key) REFERENCES attribution_commit_sessions(attribution_commit_id, note_session_key) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_attribution_commit_session_hashes_session_hash
  ON attribution_commit_session_hashes(session_hash, attribution_commit_id);

-- Harness session starts and ends reported by the CLI hooks.
CREATE TABLE IF NOT EXISTS attribution_agent_sessions (
  ownership_scope_key TEXT NOT NULL,
  harness_key TEXT NOT NULL,
  session_hash TEXT NOT NULL,
  owner_user_id TEXT,
  owner_service_account_id TEXT,
  repository TEXT,
  branch TEXT,
  start_head_sha TEXT,
  end_head_sha TEXT,
  harness_version TEXT,
  started_at BIGINT,
  ended_at BIGINT,
  last_observed_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  PRIMARY KEY (ownership_scope_key, harness_key, session_hash),
  CHECK (started_at IS NOT NULL OR ended_at IS NOT NULL),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_attribution_agent_sessions_started
  ON attribution_agent_sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_attribution_agent_sessions_ended
  ON attribution_agent_sessions(ended_at);

-- A hook-reported session's id hashed under every session namespace the
-- gateway records requests under, accumulated from the session's events.
-- `agent_session_hash` is the session row's `session_hash` key.
CREATE TABLE IF NOT EXISTS attribution_agent_session_hashes (
  ownership_scope_key TEXT NOT NULL,
  harness_key TEXT NOT NULL,
  agent_session_hash TEXT NOT NULL,
  session_hash TEXT NOT NULL,
  PRIMARY KEY (ownership_scope_key, harness_key, agent_session_hash, session_hash),
  FOREIGN KEY (ownership_scope_key, harness_key, agent_session_hash) REFERENCES attribution_agent_sessions(ownership_scope_key, harness_key, session_hash) ON DELETE CASCADE
);

-- Merged pull requests reported by `oceans hook prs`. A later observation
-- replaces the row in place, together with its commit list.
CREATE TABLE IF NOT EXISTS attribution_pull_requests (
  attribution_pull_request_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_user_id TEXT,
  owner_service_account_id TEXT,
  repository TEXT NOT NULL,
  number BIGINT NOT NULL CHECK (number > 0),
  title TEXT NOT NULL,
  url TEXT,
  base_ref TEXT,
  head_ref TEXT,
  merge_commit_sha TEXT,
  merged_at BIGINT NOT NULL,
  author_user_id TEXT,
  author_email_hash TEXT,
  observed_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (ownership_scope_key, repository, number),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (author_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_merged
  ON attribution_pull_requests(merged_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_owner_user_merged
  ON attribution_pull_requests(owner_user_id, merged_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_author_user_merged
  ON attribution_pull_requests(author_user_id, merged_at);

CREATE TABLE IF NOT EXISTS attribution_pull_request_commits (
  attribution_pull_request_id TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  ordinal BIGINT NOT NULL CHECK (ordinal >= 0),
  subject TEXT,
  authored_at BIGINT,
  author_user_id TEXT,
  author_email_hash TEXT,
  PRIMARY KEY (attribution_pull_request_id, commit_sha),
  FOREIGN KEY (attribution_pull_request_id) REFERENCES attribution_pull_requests(attribution_pull_request_id) ON DELETE CASCADE,
  FOREIGN KEY (author_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_attribution_pull_request_commits_sha
  ON attribution_pull_request_commits(commit_sha);

-- Attribution looks up gateway requests by their hashed session id. The runner
-- applies each migration in a transaction, so the build cannot be CONCURRENTLY
-- and blocks writes to agent_session_requests until it finishes.
-- On a large table, plan the upgrade for a quiet time.
CREATE INDEX IF NOT EXISTS idx_agent_session_requests_normalized_session
  ON agent_session_requests(normalized_session_id, occurred_at);
