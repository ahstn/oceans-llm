ALTER TABLE oauth_providers
    ADD COLUMN allowed_email_domains_json TEXT NOT NULL DEFAULT '[]';
