-- Pull requests that are open or closed without merging, their lifecycle and their revert
-- links. `merged_at` becomes nullable, which SQLite only allows by rebuilding the table;
-- `attribution_pull_request_commits` references it, so this runs with foreign keys off
-- (LIBSQL_REBUILT_PARENTS) and the commit rows keep pointing at the same ids.
--
-- `state` is `open`, `closed` (without merging) or `merged`; every stored row so far was
-- merged. `opened_at`, `closed_at` and `last_activity_at` (the forge's last change, such as a
-- review; not the sync time) are indexed for the report window. `details` is a JSON
-- object the gateway writes and reads: draft flag, first review time, review and comment
-- counts, size, and the revert link with its evidence. It never holds reviewer logins, file
-- paths or description text.
CREATE TABLE attribution_pull_requests_v82 (
  attribution_pull_request_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_user_id TEXT,
  owner_service_account_id TEXT,
  repository TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  title TEXT NOT NULL,
  url TEXT,
  base_ref TEXT,
  head_ref TEXT,
  merge_commit_sha TEXT,
  merged_at INTEGER,
  author_user_id TEXT,
  author_email_hash TEXT,
  observed_at INTEGER NOT NULL,
  state TEXT NOT NULL DEFAULT 'merged' CHECK (state IN ('open', 'closed', 'merged')),
  opened_at INTEGER,
  closed_at INTEGER,
  details TEXT,
  last_activity_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (ownership_scope_key, repository, number),
  CHECK (state <> 'merged' OR merged_at IS NOT NULL),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (author_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

INSERT INTO attribution_pull_requests_v82 (
  attribution_pull_request_id, ownership_scope_key, owner_user_id, owner_service_account_id,
  repository, number, title, url, base_ref, head_ref, merge_commit_sha, merged_at,
  author_user_id, author_email_hash, observed_at, state, opened_at, closed_at, details,
  last_activity_at, created_at, updated_at
)
SELECT
  attribution_pull_request_id, ownership_scope_key, owner_user_id, owner_service_account_id,
  repository, number, title, url, base_ref, head_ref, merge_commit_sha, merged_at,
  author_user_id, author_email_hash, observed_at, 'merged', NULL, merged_at, NULL,
  merged_at, created_at, updated_at
FROM attribution_pull_requests;

DROP TABLE attribution_pull_requests;
ALTER TABLE attribution_pull_requests_v82 RENAME TO attribution_pull_requests;

CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_merged
  ON attribution_pull_requests(merged_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_owner_user_merged
  ON attribution_pull_requests(owner_user_id, merged_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_author_user_merged
  ON attribution_pull_requests(author_user_id, merged_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_opened
  ON attribution_pull_requests(opened_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_closed
  ON attribution_pull_requests(closed_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_last_activity
  ON attribution_pull_requests(last_activity_at);
