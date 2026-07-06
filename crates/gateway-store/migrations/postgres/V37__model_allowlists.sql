CREATE TABLE IF NOT EXISTS model_allowlist_users (
  model_id TEXT NOT NULL,
  normalized_email TEXT NOT NULL,
  PRIMARY KEY (model_id, normalized_email),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS model_allowlist_users_email_idx
  ON model_allowlist_users (normalized_email, model_id);

CREATE TABLE IF NOT EXISTS model_allowlist_teams (
  model_id TEXT NOT NULL,
  team_key TEXT NOT NULL,
  PRIMARY KEY (model_id, team_key),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS model_allowlist_teams_team_key_idx
  ON model_allowlist_teams (team_key, model_id);
