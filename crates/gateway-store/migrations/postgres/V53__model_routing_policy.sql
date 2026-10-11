ALTER TABLE gateway_models ADD COLUMN routing_policy_json TEXT;

CREATE TABLE model_routing_cursors (
    pool_key TEXT PRIMARY KEY,
    next_index BIGINT NOT NULL DEFAULT 0 CHECK (next_index >= 0)
);

-- Route rows can be replaced during config seeding. Eligibility is checked
-- against current route fingerprints before a stored binding is reused.
CREATE TABLE model_route_bindings (
    model_id TEXT NOT NULL,
    affinity_key TEXT NOT NULL,
    route_id TEXT NOT NULL,
    route_fingerprint TEXT NOT NULL,
    binding_token TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    PRIMARY KEY (model_id, affinity_key)
);
CREATE INDEX model_route_bindings_expiry_idx ON model_route_bindings (expires_at);

CREATE TABLE response_route_origins (
    owner_key TEXT NOT NULL,
    response_id_hash TEXT NOT NULL,
    model_id TEXT NOT NULL,
    route_id TEXT NOT NULL,
    route_fingerprint TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    PRIMARY KEY (owner_key, response_id_hash)
);
CREATE INDEX response_route_origins_expiry_idx ON response_route_origins (expires_at);
