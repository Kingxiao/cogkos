//! Evolution engine for CogKOS

pub mod auto_switch;
pub mod bayesian;
pub mod conflict;
pub mod decay;
pub mod dedicated_model;
pub mod engine;
pub mod health_monitor;
pub mod insight_extraction;
pub mod paradigm;
pub mod paradigm_shift;

// Re-export with explicit handling to avoid ambiguity
pub use crate::models::{ShiftRecord, ShiftResult};
pub use auto_switch::{
    AutoSwitchConfig, AutoSwitchController, AutoSwitchStatus, EvolutionAction,
    EvolutionAutoSwitchIntegration, RolloutPhase, RolloutReason, SwitchRecommendation, TestMetrics,
};
pub use bayesian::*;
pub use conflict::*;
pub use decay::*;
pub use dedicated_model::*;
pub use engine::*;
pub use health_monitor::*;

// Avoid ambiguous re-exports by using explicit paths
#[allow(ambiguous_glob_reexports)]
pub use paradigm::{
    AnomalyConfig as ParadigmAnomalyConfig, AnomalyDetector as ParadigmAnomalyDetector,
    LlmSandbox as ParadigmLlmSandbox,
};
#[allow(ambiguous_glob_reexports)]
pub use paradigm_shift::{
    ABTestConfig, ABTestFramework, ABTestRecommendation, ABTestResult, Anomaly, AnomalyAssessment,
    AnomalyConfig as ParadigmShiftAnomalyConfig, AnomalyDetectionResult,
    AnomalyDetector as ParadigmShiftAnomalyDetector, AnomalyRecommendation, AnomalyType, Framework,
    FrameworkSwitchManager, IsolationConfig as ParadigmShiftIsolationConfig,
    LlmSandbox as ParadigmShiftLlmSandbox, OntologyDefinition, ParadigmShiftEngine,
    ParadigmShiftError, ParadigmShiftWorkflowResult, ResolutionRule, SandboxMetrics, SandboxReport,
    SandboxTestResult, SandboxTestType, TestOutcome,
};
