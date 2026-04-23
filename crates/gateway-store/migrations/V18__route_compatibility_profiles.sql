ALTER TABLE model_routes
  ADD COLUMN compatibility_json TEXT NOT NULL DEFAULT '{}';
