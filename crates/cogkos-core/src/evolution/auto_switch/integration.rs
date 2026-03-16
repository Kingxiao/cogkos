//! Evolution engine integration for auto-switching

use super::*;

/// Integration with evolution engine
pub struct EvolutionAutoSwitchIntegration {
    controller: AutoSwitchController,
    evolution_state: EvolutionEngineState,
}

impl EvolutionAutoSwitchIntegration {
    pub fn new() -> Self {
        Self {
            controller: AutoSwitchController::new(),
            evolution_state: EvolutionEngineState::default(),
        }
    }

    pub fn with_controller(mut self, controller: AutoSwitchController) -> Self {
        self.controller = controller;
        self
    }

    /// Process evolution tick and determine actions
    pub fn process_tick(&mut self, anomaly_counter: u32, threshold: u32) -> EvolutionAction {
        // Check if anomaly threshold reached
        if anomaly_counter >= threshold {
            self.evolution_state.mode = crate::models::EvolutionMode::ParadigmShift;
            return EvolutionAction::InitiateParadigmShift;
        }

        // Check if we should switch based on A/B test
        if let Some(recommendation) = self.controller.should_switch() {
            return EvolutionAction::RecommendSwitch(recommendation);
        }

        // Check for rollback
        if let Some(reason) = self.controller.should_rollback() {
            return EvolutionAction::Rollback(reason);
        }

        EvolutionAction::ContinueMonitoring
    }

    /// Record test outcome
    pub fn record_outcome(
        &mut self,
        variant: &str,
        correct: bool,
        latency_ms: f64,
        had_conflict: bool,
    ) {
        self.controller
            .record_outcome(variant, correct, latency_ms, had_conflict);
    }

    /// Get controller reference
    pub fn controller(&self) -> &AutoSwitchController {
        &self.controller
    }

    /// Get controller mutable reference
    pub fn controller_mut(&mut self) -> &mut AutoSwitchController {
        &mut self.controller
    }
}

impl Default for EvolutionAutoSwitchIntegration {
    fn default() -> Self {
        Self::new()
    }
}
