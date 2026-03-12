PRAGMA foreign_keys = OFF;

CREATE TABLE IF NOT EXISTS model_pricing (
  model_pricing_id TEXT PRIMARY KEY,
  pricing_provider_id TEXT NOT NULL,
  pricing_model_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  input_cost_per_million_tokens_10000 INTEGER,
  output_cost_per_million_tokens_10000 INTEGER,
  cache_read_cost_per_million_tokens_10000 INTEGER,
  cache_write_cost_per_million_tokens_10000 INTEGER,
  input_audio_cost_per_million_tokens_10000 INTEGER,
  output_audio_cost_per_million_tokens_10000 INTEGER,
  release_date TEXT NOT NULL,
  last_updated TEXT NOT NULL,
  effective_start_at INTEGER NOT NULL,
  effective_end_at INTEGER,
  limits_json TEXT NOT NULL DEFAULT '{}',
  modalities_json TEXT NOT NULL DEFAULT '{}',
  provenance_source TEXT NOT NULL,
  provenance_etag TEXT,
  provenance_fetched_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS model_pricing_provider_model_start_uidx
  ON model_pricing (pricing_provider_id, pricing_model_id, effective_start_at);

CREATE UNIQUE INDEX IF NOT EXISTS model_pricing_active_uidx
  ON model_pricing (pricing_provider_id, pricing_model_id)
  WHERE effective_end_at IS NULL;

CREATE INDEX IF NOT EXISTS model_pricing_lookup_idx
  ON model_pricing (pricing_provider_id, pricing_model_id, effective_start_at, effective_end_at);

CREATE TABLE IF NOT EXISTS usage_cost_event_duplicates_archive (
  archived_duplicate_id TEXT PRIMARY KEY,
  original_usage_event_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_id TEXT,
  original_estimated_cost_10000 INTEGER NOT NULL CHECK (original_estimated_cost_10000 >= 0),
  occurred_at INTEGER NOT NULL,
  archived_at INTEGER NOT NULL
);

CREATE TABLE usage_cost_events_v8 (
  usage_event_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  actor_user_id TEXT,
  model_id TEXT,
  provider_key TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  total_tokens INTEGER,
  provider_usage_json TEXT NOT NULL DEFAULT '{}',
  pricing_status TEXT NOT NULL CHECK (
    pricing_status IN ('priced', 'unpriced', 'usage_missing', 'legacy_estimated')
  ),
  unpriced_reason TEXT,
  pricing_row_id TEXT,
  pricing_provider_id TEXT,
  pricing_model_id TEXT,
  pricing_source TEXT,
  pricing_source_etag TEXT,
  pricing_source_fetched_at INTEGER,
  pricing_last_updated TEXT,
  input_cost_per_million_tokens_10000 INTEGER,
  output_cost_per_million_tokens_10000 INTEGER,
  computed_cost_10000 INTEGER NOT NULL CHECK (computed_cost_10000 >= 0),
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (actor_user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE SET NULL,
  FOREIGN KEY (pricing_row_id) REFERENCES model_pricing(model_pricing_id) ON DELETE SET NULL
);

WITH ranked_usage_cost_events AS (
  SELECT
    usage_event_id,
    request_id,
    CASE
      WHEN user_id IS NOT NULL THEN 'user:' || user_id
      WHEN team_id IS NOT NULL THEN 'team:' || team_id || ':actor:none'
      ELSE 'unknown:unowned'
    END AS ownership_scope_key,
    api_key_id,
    user_id,
    team_id,
    model_id,
    estimated_cost_10000,
    occurred_at,
    ROW_NUMBER() OVER (
      PARTITION BY request_id,
      CASE
        WHEN user_id IS NOT NULL THEN 'user:' || user_id
        WHEN team_id IS NOT NULL THEN 'team:' || team_id || ':actor:none'
        ELSE 'unknown:unowned'
      END
      ORDER BY occurred_at ASC, usage_event_id ASC
    ) AS row_num
  FROM usage_cost_events
)
INSERT INTO usage_cost_events_v8 (
  usage_event_id,
  request_id,
  ownership_scope_key,
  api_key_id,
  user_id,
  team_id,
  actor_user_id,
  model_id,
  provider_key,
  upstream_model,
  prompt_tokens,
  completion_tokens,
  total_tokens,
  provider_usage_json,
  pricing_status,
  unpriced_reason,
  pricing_row_id,
  pricing_provider_id,
  pricing_model_id,
  pricing_source,
  pricing_source_etag,
  pricing_source_fetched_at,
  pricing_last_updated,
  input_cost_per_million_tokens_10000,
  output_cost_per_million_tokens_10000,
  computed_cost_10000,
  occurred_at
)
SELECT
  usage_event_id,
  request_id,
  ownership_scope_key,
  api_key_id,
  user_id,
  team_id,
  NULL,
  model_id,
  'legacy',
  'legacy',
  NULL,
  NULL,
  NULL,
  '{}',
  'legacy_estimated',
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  estimated_cost_10000,
  occurred_at
FROM ranked_usage_cost_events
WHERE row_num = 1;

WITH ranked_usage_cost_events AS (
  SELECT
    usage_event_id,
    request_id,
    CASE
      WHEN user_id IS NOT NULL THEN 'user:' || user_id
      WHEN team_id IS NOT NULL THEN 'team:' || team_id || ':actor:none'
      ELSE 'unknown:unowned'
    END AS ownership_scope_key,
    api_key_id,
    user_id,
    team_id,
    model_id,
    estimated_cost_10000,
    occurred_at,
    ROW_NUMBER() OVER (
      PARTITION BY request_id,
      CASE
        WHEN user_id IS NOT NULL THEN 'user:' || user_id
        WHEN team_id IS NOT NULL THEN 'team:' || team_id || ':actor:none'
        ELSE 'unknown:unowned'
      END
      ORDER BY occurred_at ASC, usage_event_id ASC
    ) AS row_num
  FROM usage_cost_events
)
INSERT INTO usage_cost_event_duplicates_archive (
  archived_duplicate_id,
  original_usage_event_id,
  request_id,
  ownership_scope_key,
  api_key_id,
  user_id,
  team_id,
  model_id,
  original_estimated_cost_10000,
  occurred_at,
  archived_at
)
SELECT
  lower(hex(randomblob(16))),
  usage_event_id,
  request_id,
  ownership_scope_key,
  api_key_id,
  user_id,
  team_id,
  model_id,
  estimated_cost_10000,
  occurred_at,
  unixepoch()
FROM ranked_usage_cost_events
WHERE row_num > 1;

DROP TABLE usage_cost_events;
ALTER TABLE usage_cost_events_v8 RENAME TO usage_cost_events;

CREATE UNIQUE INDEX IF NOT EXISTS usage_cost_events_request_scope_uidx
  ON usage_cost_events (request_id, ownership_scope_key);

CREATE INDEX IF NOT EXISTS usage_cost_events_user_time_idx
  ON usage_cost_events (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_team_time_idx
  ON usage_cost_events (team_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_pricing_status_idx
  ON usage_cost_events (pricing_status, occurred_at);

PRAGMA foreign_keys = ON;
