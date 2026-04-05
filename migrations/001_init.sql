-- Initial schema for CogKOS
-- PostgreSQL 16 compatible

-- Enable required extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- Epistemic Claims table (核心知识表)
CREATE TABLE epistemic_claims (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id TEXT NOT NULL,
    content TEXT NOT NULL,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.5 CHECK (confidence >= 0 AND confidence <= 1),
    consolidation_stage TEXT NOT NULL DEFAULT 'FastTrack',
    claimant JSONB NOT NULL,
    provenance JSONB NOT NULL,
    access_envelope JSONB NOT NULL DEFAULT '{}',
    activation_weight DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}',
    
    -- Composite indexes
    CONSTRAINT valid_tenant CHECK (tenant_id ~ '^[a-z0-9_-]+$')
);

-- Indexes for performance
CREATE INDEX idx_claims_tenant ON epistemic_claims(tenant_id);
CREATE INDEX idx_claims_tenant_created ON epistemic_claims(tenant_id, created_at DESC);
CREATE INDEX idx_claims_tenant_confidence ON epistemic_claims(tenant_id, confidence DESC);
CREATE INDEX idx_claims_tenant_stage ON epistemic_claims(tenant_id, consolidation_stage);
CREATE INDEX idx_claims_expires ON epistemic_claims(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_claims_gin_metadata ON epistemic_claims USING GIN(metadata);

-- GIN index for full-text search (Phase 3+)
-- CREATE INDEX idx_claims_content_search ON epistemic_claims USING GIN(to_tsvector('simple', content));

-- Conflict Records table (冲突记录表)
CREATE TABLE conflict_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id TEXT NOT NULL,
    claim_a_id UUID NOT NULL REFERENCES epistemic_claims(id) ON DELETE CASCADE,
    claim_b_id UUID NOT NULL REFERENCES epistemic_claims(id) ON DELETE CASCADE,
    conflict_type TEXT NOT NULL,
    severity DOUBLE PRECISION NOT NULL CHECK (severity >= 0 AND severity <= 1),
    description TEXT NOT NULL DEFAULT '',
    detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ,
    resolution JSONB,
    
    CONSTRAINT different_claims CHECK (claim_a_id != claim_b_id)
);

CREATE INDEX idx_conflicts_tenant ON conflict_records(tenant_id);
CREATE INDEX idx_conflicts_claim_a ON conflict_records(claim_a_id);
CREATE INDEX idx_conflicts_claim_b ON conflict_records(claim_b_id);
CREATE INDEX idx_conflicts_unresolved ON conflict_records(tenant_id) WHERE resolved_at IS NULL;

-- API Keys table (认证表)
CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_hash TEXT NOT NULL UNIQUE,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    permissions TEXT[] NOT NULL DEFAULT '{read}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_used_at TIMESTAMPTZ,
    
    CONSTRAINT valid_permissions CHECK (permissions <@ ARRAY['read', 'write', 'admin', 'delete'])
);

CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX idx_api_keys_tenant ON api_keys(tenant_id);

-- Query Cache table (查询缓存表)
CREATE TABLE query_cache (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id TEXT NOT NULL,
    query_hash BIGINT NOT NULL,
    response JSONB NOT NULL,
    confidence DOUBLE PRECISION NOT NULL,
    hit_count BIGINT NOT NULL DEFAULT 0,
    success_count BIGINT NOT NULL DEFAULT 0,
    last_used TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ttl_seconds INT NOT NULL DEFAULT 3600,
    
    UNIQUE(tenant_id, query_hash)
);

CREATE INDEX idx_cache_tenant_hash ON query_cache(tenant_id, query_hash);
CREATE INDEX idx_cache_last_used ON query_cache(last_used);

-- Subscriptions table (外部知识订阅表)
CREATE TABLE subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    source_type TEXT NOT NULL, -- rss, api, scraper, search
    source_config JSONB NOT NULL,
    schedule TEXT NOT NULL DEFAULT '0 */6 * * *', -- cron expression
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TIMESTAMPTZ,
    last_run_status TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_subscriptions_tenant ON subscriptions(tenant_id);
CREATE INDEX idx_subscriptions_enabled ON subscriptions(tenant_id) WHERE enabled = TRUE;

-- Audit Log table (审计日志表) - matches AuditEntry structure
CREATE TABLE audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    category TEXT NOT NULL DEFAULT 'System',
    severity TEXT NOT NULL DEFAULT 'Info',
    action TEXT NOT NULL,
    
    -- Actor fields
    actor_user_id TEXT,
    actor_api_key_hash TEXT,
    actor_service_id TEXT,
    actor_ip_address INET,
    actor_user_agent TEXT,
    
    -- Target fields
    target_resource_type TEXT,
    target_resource_id TEXT,
    target_metadata JSONB DEFAULT '{}',
    
    -- Outcome
    outcome TEXT NOT NULL DEFAULT 'Success',
    error_message TEXT,
    
    -- Additional details
    details JSONB DEFAULT '{}',
    
    -- Tracing
    request_id UUID,
    
    -- Tenant
    tenant_id TEXT
);

-- Indexes for common query patterns
CREATE INDEX idx_audit_tenant_time ON audit_logs(tenant_id, timestamp DESC);
CREATE INDEX idx_audit_category ON audit_logs(category);
CREATE INDEX idx_audit_severity ON audit_logs(severity);
CREATE INDEX idx_audit_action ON audit_logs(action);
CREATE INDEX idx_audit_target ON audit_logs(target_resource_type, target_resource_id);
CREATE INDEX idx_audit_outcome ON audit_logs(outcome);
CREATE INDEX idx_audit_timestamp ON audit_logs(timestamp DESC);

-- Trigger function for updated_at
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Apply updated_at triggers
CREATE TRIGGER update_claims_updated_at BEFORE UPDATE ON epistemic_claims
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_subscriptions_updated_at BEFORE UPDATE ON subscriptions
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Row Level Security (RLS) policies
ALTER TABLE epistemic_claims ENABLE ROW LEVEL SECURITY;
ALTER TABLE conflict_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE query_cache ENABLE ROW LEVEL SECURITY;
ALTER TABLE subscriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_logs ENABLE ROW LEVEL SECURITY;

-- RLS policy for multi-tenancy
CREATE POLICY tenant_isolation_claims ON epistemic_claims
    USING (tenant_id = current_setting('app.current_tenant', true));

CREATE POLICY tenant_isolation_conflicts ON conflict_records
    USING (tenant_id = current_setting('app.current_tenant', true));

CREATE POLICY tenant_isolation_cache ON query_cache
    USING (tenant_id = current_setting('app.current_tenant', true));

CREATE POLICY tenant_isolation_subscriptions ON subscriptions
    USING (tenant_id = current_setting('app.current_tenant', true));

CREATE POLICY tenant_isolation_audit ON audit_logs
    USING (tenant_id = current_setting('app.current_tenant', true));
