CREATE TABLE skill_namespaces (
  user_id TEXT PRIMARY KEY REFERENCES users(user_id) ON DELETE RESTRICT,
  handle TEXT NOT NULL UNIQUE,
  created_at BIGINT NOT NULL
);

CREATE TABLE skills (
  skill_id TEXT PRIMARY KEY,
  owner_user_id TEXT NOT NULL REFERENCES skill_namespaces(user_id) ON DELETE RESTRICT,
  name TEXT NOT NULL,
  description TEXT NOT NULL,
  default_version BIGINT NOT NULL CHECK (default_version > 0),
  latest_version BIGINT NOT NULL CHECK (latest_version >= default_version AND latest_version <= 4294967295),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (owner_user_id, name)
);

CREATE TABLE skill_versions (
  skill_id TEXT NOT NULL REFERENCES skills(skill_id) ON DELETE RESTRICT,
  version BIGINT NOT NULL CHECK (version > 0 AND version <= 4294967295),
  description TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  object_key TEXT NOT NULL UNIQUE,
  archive_bytes BIGINT NOT NULL CHECK (archive_bytes >= 0),
  extracted_bytes BIGINT NOT NULL CHECK (extracted_bytes >= 0),
  file_count BIGINT NOT NULL CHECK (file_count >= 0 AND file_count <= 4294967295),
  manifest_json TEXT NOT NULL,
  files_json TEXT NOT NULL,
  instructions TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (skill_id, version)
);

CREATE INDEX skills_updated_idx ON skills(updated_at DESC, skill_id);
