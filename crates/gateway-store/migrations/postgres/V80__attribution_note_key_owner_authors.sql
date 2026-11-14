SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- A note sent with a user's API key now names that user as the author of its
-- commit and sessions (ADR "Joining by email"). Rewrite stored notes to match,
-- so commits matched by an email hash, or by another user's email, count for
-- the key owner. Notes sent with a service account key keep their email match.
UPDATE attribution_commits
SET author_user_id = owner_user_id,
    author_email_hash = NULL
WHERE owner_user_id IS NOT NULL;

UPDATE attribution_commit_sessions AS sessions
SET human_author_user_id = commits.owner_user_id,
    human_author_hash = NULL
FROM attribution_commits AS commits
WHERE commits.attribution_commit_id = sessions.attribution_commit_id
  AND commits.owner_user_id IS NOT NULL;
