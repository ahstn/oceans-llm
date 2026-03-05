CREATE TABLE IF NOT EXISTS pricing_catalog_cache (
  catalog_key TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  etag TEXT,
  fetched_at INTEGER NOT NULL,
  snapshot_json TEXT NOT NULL
);
