ALTER TABLE model_routes
ADD COLUMN capabilities_json TEXT NOT NULL DEFAULT '{"chat_completions":true,"stream":true,"embeddings":true,"tools":true,"vision":true,"json_schema":true,"developer_role":true}';
