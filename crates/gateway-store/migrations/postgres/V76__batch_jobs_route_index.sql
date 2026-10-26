-- Seeding keeps model routes that a batch job references and deletes the rest. Index
-- batch_jobs.route_id so those EXISTS checks do not scan the batch history.
-- The runner wraps each migration in a transaction, so the build cannot be CONCURRENTLY and
-- blocks writes to batch_jobs while it runs. The timeouts bound that pause. On a batch_jobs
-- table too large to index within them, build the index online before upgrading:
--   CREATE INDEX CONCURRENTLY IF NOT EXISTS batch_jobs_route_idx ON batch_jobs (route_id);
-- This migration then finds it and does nothing.
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
CREATE INDEX IF NOT EXISTS batch_jobs_route_idx ON batch_jobs (route_id);
