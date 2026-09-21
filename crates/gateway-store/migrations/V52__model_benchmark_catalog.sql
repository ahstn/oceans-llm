CREATE TABLE model_benchmark_bindings (
  model_id TEXT NOT NULL,
  source TEXT NOT NULL,
  source_model_id TEXT NOT NULL,
  PRIMARY KEY (model_id, source),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX model_benchmark_bindings_source_idx
  ON model_benchmark_bindings (source, source_model_id);

CREATE TABLE model_benchmark_scores (
  model_id TEXT NOT NULL,
  source TEXT NOT NULL,
  metric_key TEXT NOT NULL,
  label TEXT NOT NULL,
  value REAL NOT NULL,
  unit TEXT NOT NULL,
  benchmark_version TEXT NOT NULL,
  source_model_id TEXT NOT NULL,
  source_url TEXT NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (model_id, source, metric_key),
  FOREIGN KEY (model_id, source)
    REFERENCES model_benchmark_bindings(model_id, source)
    ON DELETE CASCADE
);

CREATE INDEX model_benchmark_scores_source_idx
  ON model_benchmark_scores (source, metric_key);

CREATE TABLE benchmark_sync_state (
  source TEXT PRIMARY KEY,
  benchmark_version TEXT NOT NULL,
  last_successful_refresh_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
