-- Opaque keys include the route, provider identity, and caller credential scope.
-- No route foreign key: config seeding can replace route rows.
CREATE TABLE model_route_cooldowns (
    cooldown_key TEXT PRIMARY KEY,
    expires_at BIGINT NOT NULL
);
CREATE INDEX model_route_cooldowns_expiry_idx ON model_route_cooldowns (expires_at);
