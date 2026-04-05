-- Align migration schema with PostgresStore expectations
-- Adds missing columns and tables that were in init_schema() but not in migrations

-- ========== epistemic_claims: add missing columns ==========
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS access_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS last_accessed TIMESTAMPTZ;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS t_valid_start TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS t_valid_end TIMESTAMPTZ;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS t_known TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS vector_id UUID;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS last_prediction_error FLOAT;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS needs_revalidation BOOLEAN DEFAULT FALSE;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS durability FLOAT NOT NULL DEFAULT 1.0;

-- ========== conflict_records: add missing columns ==========
ALTER TABLE conflict_records ADD COLUMN IF NOT EXISTS resolution_status TEXT DEFAULT 'open';
ALTER TABLE conflict_records ADD COLUMN IF NOT EXISTS elevated_insight_id UUID;
ALTER TABLE conflict_records ADD COLUMN IF NOT EXISTS resolution_note TEXT;

-- ========== knowledge_gaps table (missing entirely) ==========
CREATE TABLE IF NOT EXISTS knowledge_gaps (
    gap_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id TEXT NOT NULL,
    domain TEXT NOT NULL,
    description TEXT NOT NULL,
    priority TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    reported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    filled_at TIMESTAMPTZ,
    UNIQUE(tenant_id, domain, description)
);

CREATE INDEX IF NOT EXISTS idx_gaps_tenant ON knowledge_gaps(tenant_id);

-- ========== agent_feedbacks table (missing entirely) ==========
CREATE TABLE IF NOT EXISTS agent_feedbacks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    query_hash BIGINT,
    agent_id TEXT NOT NULL,
    success BOOLEAN NOT NULL,
    feedback_note TEXT,
    timestamp TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_feedbacks_agent ON agent_feedbacks(agent_id);

-- ========== subscriptions: add missing columns for SubscriptionStore ==========
-- PostgresStore uses: config, poll_interval_secs, claimant_template, base_confidence, error_count
-- Migration 001 uses: source_config, schedule
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS config JSONB NOT NULL DEFAULT '{}';
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS poll_interval_secs BIGINT NOT NULL DEFAULT 3600;
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS claimant_template JSONB NOT NULL DEFAULT '{}';
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS base_confidence FLOAT NOT NULL DEFAULT 0.5;
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS error_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE subscriptions ADD COLUMN IF NOT EXISTS last_polled TIMESTAMPTZ;

-- ========== query_cache: add missing column ==========
ALTER TABLE query_cache ADD COLUMN IF NOT EXISTS invalidated_by UUID;

-- ========== RLS for new tables ==========
ALTER TABLE knowledge_gaps ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_feedbacks ENABLE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation_gaps ON knowledge_gaps
    USING (tenant_id = current_setting('app.current_tenant', true));
