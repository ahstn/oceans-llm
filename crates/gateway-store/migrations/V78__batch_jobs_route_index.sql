-- Seeding keeps model routes that a batch job references and deletes the rest. Index
-- batch_jobs.route_id so those EXISTS checks do not scan the batch history.
CREATE INDEX IF NOT EXISTS batch_jobs_route_idx ON batch_jobs (route_id);
