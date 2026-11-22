-- Ledger charges move from $0.0001 units to nano-dollars, so a charge below $0.00005 is no
-- longer stored as zero. Each existing value is multiplied by 100,000, which rewrites every row.
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '15min';

-- The migration registry marks V84 as RequiresReader(84), so a gateway that still reads
-- `computed_cost_10000` refuses to start on a migrated database.
-- squawk-ignore renaming-column
ALTER TABLE usage_cost_events RENAME COLUMN computed_cost_10000 TO computed_cost_nanos;
UPDATE usage_cost_events SET computed_cost_nanos = computed_cost_nanos * 100000;
