-- 'config_service_account' marks budgets seeded from `service_accounts[*].budget`.
-- Config reloads never overwrite a manual admin edit to the active row; a manually
-- deactivated service-account budget is re-created because active keys require one.
ALTER TABLE budgets
  DROP CONSTRAINT budgets_source_kind_check;

ALTER TABLE budgets
  ADD CONSTRAINT budgets_source_kind_check
  CHECK (source_kind IN ('manual', 'config_user_override', 'config_user_default', 'config_user_model_default', 'config_service_account'));
