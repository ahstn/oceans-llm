ALTER TABLE oauth_providers
    ADD COLUMN sso_email_verification_enabled BIGINT NOT NULL DEFAULT 1 CHECK (sso_email_verification_enabled IN (0, 1));
