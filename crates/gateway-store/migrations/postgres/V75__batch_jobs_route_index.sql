-- Seeding keeps model routes that a batch job references and deletes the rest. Index
-- batch_jobs.route_id so those EXISTS checks do not scan the batch history.
-- Building the index blocks writes to batch_jobs. Fail fast rather than queue behind
-- long-running writers, but give the build itself room on a large batch history.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '15min';
CREATE INDEX IF NOT EXISTS batch_jobs_route_idx ON batch_jobs (route_id);
