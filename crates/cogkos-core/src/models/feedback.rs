use serde::{Deserialize, Serialize};

/// Agent feedback on query results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFeedback {
    pub query_hash: u64,
    pub agent_id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback_note: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl AgentFeedback {
    /// Create new feedback
    pub fn new(query_hash: u64, agent_id: impl Into<String>, success: bool) -> Self {
        Self {
            query_hash,
            agent_id: agent_id.into(),
            success,
            feedback_note: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Add feedback note
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.feedback_note = Some(note.into());
        self
    }
}
