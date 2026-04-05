-- Prediction history table (replaces ClickHouse)
CREATE TABLE IF NOT EXISTS prediction_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id TEXT NOT NULL,
    claim_id UUID,
    predicted_probability FLOAT,
    actual_result BOOLEAN,
    prediction_error FLOAT,
    predicted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    validated_at TIMESTAMPTZ,
    feedback_source TEXT,
    claim_content TEXT,
    claim_type TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_pred_history_tenant ON prediction_history(tenant_id, predicted_at DESC);
CREATE INDEX IF NOT EXISTS idx_pred_history_claim ON prediction_history(claim_id);
