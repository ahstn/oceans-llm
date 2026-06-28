ALTER TABLE oauth_providers
    ADD COLUMN sso_email_verification_enabled INTEGER NOT NULL DEFAULT 1;
