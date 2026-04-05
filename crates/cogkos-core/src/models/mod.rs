use chrono::Datelike;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod access;
pub mod claim;
pub mod conflict;
pub mod evolution;
pub mod feedback;
pub mod query;
pub mod subscription;

pub use access::*;
pub use claim::*;
pub use conflict::*;
pub use evolution::*;
pub use feedback::*;
pub use query::*;
pub use subscription::*;

/// Common metadata for all models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub version: u32,
}

impl Default for Metadata {
    fn default() -> Self {
        let now = chrono::Utc::now();
        Self {
            created_at: now,
            updated_at: now,
            version: 1,
        }
    }
}

/// JSON helper type for flexible metadata
pub type JsonValue = serde_json::Value;
pub type JsonMap = HashMap<String, JsonValue>;

/// Generic ID type alias
pub type Id = uuid::Uuid;

/// Claim metadata stub for backward compatibility
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaimMetadata {
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

/// Time bucket for temporal aggregation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeBucket {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl TimeBucket {
    /// Convert timestamp to bucket string
    pub fn from_timestamp(timestamp: chrono::DateTime<chrono::Utc>, bucket: Self) -> String {
        match bucket {
            TimeBucket::Minute => timestamp.format("%Y-%m-%d-%H-%M").to_string(),
            TimeBucket::Hour => timestamp.format("%Y-%m-%d-%H").to_string(),
            TimeBucket::Day => timestamp.format("%Y-%m-%d").to_string(),
            TimeBucket::Week => {
                let week = timestamp.iso_week();
                format!("{}-W{:02}", timestamp.year(), week.week())
            }
            TimeBucket::Month => timestamp.format("%Y-%m").to_string(),
            TimeBucket::Year => timestamp.format("%Y").to_string(),
        }
    }
}
