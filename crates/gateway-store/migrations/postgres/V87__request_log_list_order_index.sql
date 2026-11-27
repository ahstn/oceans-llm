SET LOCAL lock_timeout = '5s';
-- Longer than other migrations' 60s: the index build reads every request_logs row.
SET LOCAL statement_timeout = '15min';

-- The admin request log list orders by (occurred_at DESC, request_log_id DESC) and reads
-- one page. `occurred_at` had only a BRIN index, which cannot return rows in order, so
-- every page scanned and sorted the whole table (about 700 MB at 1M rows). This btree
-- matches the list order, so a page reads only its own rows. The BRIN index stays for
-- range scans such as the retention purge.
--
-- The runner applies each migration in a transaction, so this build cannot be
-- CONCURRENTLY and blocks writes to request_logs, and so request logging, until it
-- finishes. On a large table, first run the CREATE INDEX below with CONCURRENTLY outside
-- a transaction and check that `pg_index.indisvalid` is true (a failed concurrent build
-- leaves an invalid index that this migration would skip).
CREATE INDEX IF NOT EXISTS request_logs_occurred_at_list_idx
  ON request_logs (occurred_at DESC, request_log_id DESC);
