CREATE TABLE batch_jobs (
  batch_id TEXT PRIMARY KEY,
  idempotency_key TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  service_account_id TEXT,
  model_id TEXT NOT NULL,
  model_key TEXT NOT NULL,
  resolved_model_key TEXT NOT NULL,
  route_id TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  endpoint TEXT NOT NULL CHECK (endpoint IN ('chat_completions', 'responses', 'embeddings')),
  status TEXT NOT NULL CHECK (status IN (
    'queued', 'submitting', 'submission_unknown', 'validating', 'in_progress', 'finalizing',
    'completed', 'failed', 'expired', 'cancel_requested', 'cancelling', 'cancelled'
  )),
  provider_batch_id TEXT,
  request_count INTEGER NOT NULL CHECK (request_count > 0),
  completed_count INTEGER NOT NULL DEFAULT 0 CHECK (completed_count >= 0),
  failed_count INTEGER NOT NULL DEFAULT 0 CHECK (failed_count >= 0),
  cost_usd_10000 INTEGER CHECK (cost_usd_10000 >= 0),
  pricing_status TEXT NOT NULL DEFAULT 'pending' CHECK (pricing_status IN (
    'pending', 'priced', 'partially_priced', 'unpriced', 'provider_reported'
  )),
  provider_usage_json TEXT,
  error_json TEXT,
  created_at INTEGER NOT NULL,
  submitted_at INTEGER,
  completed_at INTEGER,
  updated_at INTEGER NOT NULL,
  next_poll_at INTEGER,
  lease_owner TEXT,
  lease_expires_at INTEGER,
  provider_context_json TEXT NOT NULL,
  UNIQUE (api_key_id, idempotency_key),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE SET NULL,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE RESTRICT,
  FOREIGN KEY (route_id) REFERENCES model_routes(id) ON DELETE RESTRICT,
  FOREIGN KEY (provider_key) REFERENCES providers(provider_key) ON DELETE RESTRICT
);

CREATE INDEX batch_jobs_created_at_idx ON batch_jobs (created_at DESC, batch_id DESC);
CREATE INDEX batch_jobs_user_time_idx ON batch_jobs (user_id, created_at DESC);
CREATE INDEX batch_jobs_service_account_time_idx ON batch_jobs (service_account_id, created_at DESC);
CREATE INDEX batch_jobs_work_idx ON batch_jobs (status, next_poll_at, lease_expires_at);
CREATE UNIQUE INDEX batch_jobs_provider_id_uidx
  ON batch_jobs (provider_key, provider_batch_id)
  WHERE provider_batch_id IS NOT NULL;

CREATE TABLE batch_items (
  batch_item_id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  custom_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'succeeded', 'failed')),
  request_body_json TEXT NOT NULL,
  response_body_json TEXT,
  error_json TEXT,
  provider_request_id TEXT,
  provider_usage_json TEXT,
  cost_usd_10000 INTEGER CHECK (cost_usd_10000 >= 0),
  completed_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (batch_id, custom_id),
  FOREIGN KEY (batch_id) REFERENCES batch_jobs(batch_id) ON DELETE CASCADE
);

CREATE INDEX batch_items_batch_status_idx ON batch_items (batch_id, status, custom_id);
