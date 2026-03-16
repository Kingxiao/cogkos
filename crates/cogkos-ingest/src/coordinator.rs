//! Ingest Coordinator - Manages document ingestion workflow

/// Coordinator for document ingestion pipeline
pub struct IngestCoordinator;

impl IngestCoordinator {
    /// Create new coordinator
    pub fn new() -> Self {
        Self
    }
}

impl Default for IngestCoordinator {
    fn default() -> Self {
        Self::new()
    }
}
