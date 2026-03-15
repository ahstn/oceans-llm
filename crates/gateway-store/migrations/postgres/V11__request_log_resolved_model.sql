ALTER TABLE request_logs
ADD COLUMN IF NOT EXISTS resolved_model_key TEXT;

UPDATE request_logs
SET resolved_model_key = model_key
WHERE resolved_model_key IS NULL;
