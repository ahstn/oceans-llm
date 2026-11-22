ALTER TABLE usage_cost_events RENAME COLUMN computed_cost_10000 TO computed_cost_nanos;
UPDATE usage_cost_events SET computed_cost_nanos = computed_cost_nanos * 100000;
