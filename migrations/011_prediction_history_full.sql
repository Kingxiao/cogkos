-- Add missing columns to prediction_history for full PredictionHistoryStore support
-- Migration 003 created base table; this adds columns needed by PostgresPredictionStore

ALTER TABLE prediction_history
    ADD COLUMN IF NOT EXISTS record_id TEXT,
    ADD COLUMN IF NOT EXISTS validation_id TEXT,
    ADD COLUMN IF NOT EXISTS squared_error FLOAT DEFAULT 0,
    ADD COLUMN IF NOT EXISTS confidence_adjustment FLOAT DEFAULT 0;

-- Backfill record_id from id where null
UPDATE prediction_history SET record_id = id::text WHERE record_id IS NULL;

-- Index for high-error queries
CREATE INDEX IF NOT EXISTS idx_pred_history_error ON prediction_history(tenant_id, prediction_error DESC);
