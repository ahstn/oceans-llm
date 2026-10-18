-- Counts a server's disable and enable transitions. Discovery reads it before
-- loading the server and refuses to record a result once it has moved, so a
-- disable followed by an enable on another replica (status back to active)
-- still invalidates a discovery that started before them.
ALTER TABLE external_mcp_servers
  ADD COLUMN status_revision BIGINT NOT NULL DEFAULT 0;
