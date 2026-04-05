-- Migration 010: Harden tenant isolation
-- Add tenant_id to agent_feedbacks table and ensure RLS policies cover all tables

-- ========== agent_feedbacks: add tenant_id column ==========
ALTER TABLE agent_feedbacks ADD COLUMN IF NOT EXISTS tenant_id TEXT NOT NULL DEFAULT 'default';

-- Index for tenant-scoped feedback queries
CREATE INDEX IF NOT EXISTS idx_feedbacks_tenant_query ON agent_feedbacks(tenant_id, query_hash);

-- RLS policy for agent_feedbacks (drop old if exists, recreate with tenant_id)
DROP POLICY IF EXISTS tenant_isolation_feedbacks ON agent_feedbacks;
CREATE POLICY tenant_isolation_feedbacks ON agent_feedbacks
    USING (tenant_id = current_setting('app.current_tenant', true));
