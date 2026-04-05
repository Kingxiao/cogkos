-- PostgreSQL Schema Upgrade: pgvector + New Columns
-- For CogKOS V2 data model

-- Enable pgvector extension
CREATE EXTENSION IF NOT EXISTS vector;

-- Add new columns to epistemic_claims
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS knowledge_type TEXT NOT NULL DEFAULT 'Experiential';
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS structured_content JSONB;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS node_type TEXT NOT NULL DEFAULT 'Entity';
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS epistemic_status TEXT NOT NULL DEFAULT 'Asserted';
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS superseded_by UUID REFERENCES epistemic_claims(id);
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS entity_refs JSONB DEFAULT '[]';
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS derived_from UUID[] DEFAULT '{}';

-- Add embedding column for vector search (if not exists)
ALTER TABLE epistemic_claims ADD COLUMN IF NOT EXISTS embedding vector(512);

-- Create indexes for new columns
CREATE INDEX IF NOT EXISTS idx_claims_knowledge_type ON epistemic_claims(knowledge_type);
CREATE INDEX IF NOT EXISTS idx_claims_node_type ON epistemic_claims(node_type);
CREATE INDEX IF NOT EXISTS idx_claims_status ON epistemic_claims(epistemic_status);
CREATE INDEX IF NOT EXISTS idx_claims_superseded ON epistemic_claims(superseded_by) WHERE superseded_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_claims_entity_refs ON epistemic_claims USING GIN(entity_refs);

-- Create HNSW index for vector search (pgvector 0.5+)
CREATE INDEX IF NOT EXISTS idx_claims_embedding ON epistemic_claims
    USING hnsw (embedding vector_cosine_ops)
    WITH (m = 16, ef_construction = 64);

-- Add comments for documentation
COMMENT ON COLUMN epistemic_claims.knowledge_type IS 'Knowledge type: Experiential, Business, or Learned';
COMMENT ON COLUMN epistemic_claims.structured_content IS 'Structured data in JSON format';
COMMENT ON COLUMN epistemic_claims.node_type IS 'Node type: Entity, Relation, Event, Attribute, or Insight';
COMMENT ON COLUMN epistemic_claims.epistemic_status IS 'Status: Asserted, Unconfirmed, Superseded, or Archived';
COMMENT ON COLUMN epistemic_claims.version IS 'Version number for Business knowledge';
COMMENT ON COLUMN epistemic_claims.superseded_by IS 'Reference to newer version of this claim';
COMMENT ON COLUMN epistemic_claims.entity_refs IS 'References to related entities';
COMMENT ON COLUMN epistemic_claims.embedding IS 'Vector embedding for semantic search';
