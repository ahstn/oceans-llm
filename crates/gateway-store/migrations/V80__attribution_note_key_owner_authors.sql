-- A note sent with a user's API key now names that user as the author of its
-- commit and sessions (ADR "Joining by email"). Rewrite stored notes to match,
-- so commits matched by an email hash, or by another user's email, count for
-- the key owner. Notes sent with a service account key keep their email match.
UPDATE attribution_commits
SET author_user_id = owner_user_id,
    author_email_hash = NULL
WHERE owner_user_id IS NOT NULL;

UPDATE attribution_commit_sessions
SET human_author_user_id = (
      SELECT owner_user_id
      FROM attribution_commits
      WHERE attribution_commits.attribution_commit_id = attribution_commit_sessions.attribution_commit_id
    ),
    human_author_hash = NULL
WHERE attribution_commit_id IN (
  SELECT attribution_commit_id FROM attribution_commits WHERE owner_user_id IS NOT NULL
);
