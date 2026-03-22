"""CogKOS quickstart — 5 lines to connect your agent to shared knowledge."""

from cogkos import CogKOS

brain = CogKOS(
    "http://localhost:3000/mcp",
    api_key="your_api_key_here",
    tenant_id="my-project",
)

# 1. Store knowledge
result = brain.learn(
    "RLS isolation must use explicit WHERE tenant_id",
    confidence=0.9,
    tags=["architecture", "security"],
    source_agent="claude-code",
)
print(f"Stored claim: {result.claim_id}")

# 1b. Store with knowledge type (authority tier)
result = brain.learn(
    "Q2 revenue target: $2M ARR",
    confidence=0.95,
    knowledge_type="Business",
    tags=["strategy", "revenue"],
)
print(f"Business knowledge: {result.claim_id}")

# 2. Query knowledge
result = brain.recall("multi-tenant data isolation best practices")
print(f"Best belief: {result.best_belief}")
print(f"Related: {len(result.related)} items")
print(f"Conflicts: {len(result.conflicts)} items")

# 3. Submit feedback
brain.feedback(result.query_hash, success=True, note="Accurate answer")

# 4. Report knowledge gap
gap = brain.report_gap(
    domain="security",
    description="Missing RBAC best practices documentation",
    priority="high",
)
print(f"Gap reported: {gap.gap_id}")

brain.close()
