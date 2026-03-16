//! Paradigm Shift Mode - Framework evolution through anomaly detection and A/B testing
//!
//! Implements the paradigm shift mode of the evolution engine:
//! 1. Anomaly detection - identify data that doesn't fit current framework
//! 2. LLM sandbox - test new frameworks in isolation
//! 3. A/B testing - compare framework performance
//! 4. Switch/rollback - safely transition between frameworks

pub mod anomaly;
pub mod engine;
pub mod llm_types;
pub mod sandbox;

pub use anomaly::*;
pub use engine::*;
pub use llm_types::*;
pub use sandbox::*;
