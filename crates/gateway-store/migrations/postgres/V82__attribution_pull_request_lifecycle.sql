-- Pull requests that are open or closed without merging, their lifecycle and their revert
-- links. `state` is `open`, `closed` (without merging) or `merged`; every stored row so far
-- was merged, so it closed when it merged. `details` is a JSON object the gateway writes and
-- reads: draft flag, first review time, review and comment counts, size, and the revert link
-- with its evidence. It never holds reviewer logins, file paths or description text.
ALTER TABLE attribution_pull_requests ALTER COLUMN merged_at DROP NOT NULL;
ALTER TABLE attribution_pull_requests
  ADD COLUMN state TEXT NOT NULL DEFAULT 'merged'
    CHECK (state IN ('open', 'closed', 'merged')),
  ADD COLUMN opened_at BIGINT,
  ADD COLUMN closed_at BIGINT,
  ADD COLUMN details TEXT,
  ADD CONSTRAINT attribution_pull_requests_merged_has_time
    CHECK (state <> 'merged' OR merged_at IS NOT NULL);
UPDATE attribution_pull_requests SET closed_at = merged_at WHERE closed_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_opened
  ON attribution_pull_requests(opened_at);
CREATE INDEX IF NOT EXISTS idx_attribution_pull_requests_closed
  ON attribution_pull_requests(closed_at);
