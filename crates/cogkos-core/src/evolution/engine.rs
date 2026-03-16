//! Evolution engine - core orchestration

use crate::models::*;
use uuid::Uuid;

/// Evolution engine for managing knowledge evolution
pub struct EvolutionEngine {
    state: EvolutionEngineState,
    config: EvolutionConfig,
}

/// Configuration for evolution
#[derive(Clone, Debug)]
pub struct EvolutionConfig {
    /// Decay lambda (per hour)
    pub decay_lambda: f64,
    /// Confidence threshold for revalidation
    pub revalidation_threshold: f64,
    /// Max age before revalidation (hours)
    pub max_age_hours: f64,
    /// Anomaly threshold for paradigm shift
    pub anomaly_threshold: u32,
    /// Minimum improvement for paradigm shift acceptance
    pub min_improvement_pct: f64,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            decay_lambda: 0.001, // 0.1% per hour
            revalidation_threshold: 0.3,
            max_age_hours: 168.0, // 1 week
            anomaly_threshold: 100,
            min_improvement_pct: 0.10, // 10%
        }
    }
}

impl EvolutionEngine {
    pub fn new(config: EvolutionConfig) -> Self {
        Self {
            state: EvolutionEngineState::default(),
            config,
        }
    }

    pub fn with_state(mut self, state: EvolutionEngineState) -> Self {
        self.state = state;
        self
    }

    /// Get current state
    pub fn state(&self) -> &EvolutionEngineState {
        &self.state
    }

    /// Tick the evolution engine (called periodically)
    pub fn tick(&mut self, signals: AnomalySignals) {
        self.state.ticks_since_last_shift += 1;

        // Check for anomaly conditions
        if self.detect_anomaly(&signals) {
            self.state.anomaly_counter += 1;

            // Check if paradigm shift threshold reached
            if self.state.anomaly_counter >= self.config.anomaly_threshold {
                self.state.mode = EvolutionMode::ParadigmShift;
            }
        } else {
            // Decay anomaly counter in normal conditions
            self.state.anomaly_counter = self.state.anomaly_counter.saturating_sub(1);
        }
    }

    /// Detect anomaly conditions
    fn detect_anomaly(&self, signals: &AnomalySignals) -> bool {
        // Signal 1: Prediction error streak
        if signals.prediction_error_streak > 5 {
            return true;
        }

        // Signal 2: High conflict density
        if signals.conflict_density_pct > 0.3 {
            return true;
        }

        // Signal 3: Declining cache hit rate
        if signals.cache_hit_rate_trend < -0.2 {
            return true;
        }

        false
    }

    /// Apply knowledge decay to a claim
    pub fn apply_decay(&self, claim: &mut EpistemicClaim, hours_elapsed: f64) {
        let new_confidence = super::decay::calculate_decay(
            claim.confidence,
            self.config.decay_lambda,
            hours_elapsed,
            claim.activation_weight,
        );

        claim.confidence = new_confidence;
        claim.activation_weight *= 0.99; // Slight decay of activation too

        // Check if revalidation needed
        claim.needs_revalidation = super::decay::needs_revalidation(
            claim.confidence,
            self.config.revalidation_threshold,
            hours_elapsed,
            self.config.max_age_hours,
        );
    }

    /// Aggregate claims into a belief
    pub fn aggregate_claims(&self, claims: &[EpistemicClaim]) -> BeliefSummary {
        let aggregated_confidence = super::bayesian::bayesian_aggregate_deduplicated(claims);

        // Get unique claim IDs
        let claim_ids: Vec<Id> = claims.iter().map(|c| c.id).collect();

        // Determine best content (highest confidence)
        let best_content = claims
            .iter()
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
            .map(|c| c.content.clone())
            .unwrap_or_default();

        // Determine consolidation stage
        let stage = if claims.len() >= 5 {
            ConsolidationStage::Insight
        } else if claims.len() >= 2 {
            ConsolidationStage::Consolidated
        } else {
            ConsolidationStage::FastTrack
        };

        BeliefSummary {
            claim_id: claims.first().map(|c| c.id),
            content: best_content,
            confidence: aggregated_confidence,
            based_on: claims.len(),
            consolidation_stage: stage,
            claim_ids,
        }
    }

    /// Consolidate multiple claims about the same topic
    pub fn consolidate_claims(&self, claims: &mut [EpistemicClaim]) -> Option<EpistemicClaim> {
        if claims.is_empty() {
            return None;
        }

        // Sort by confidence descending
        claims.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        let belief = self.aggregate_claims(claims);

        // Create consolidated claim
        let mut consolidated = claims[0].clone();
        consolidated.id = Uuid::new_v4();
        consolidated.confidence = belief.confidence;
        consolidated.consolidation_stage = belief.consolidation_stage;
        consolidated.derived_from = belief.claim_ids;

        Some(consolidated)
    }

    /// Detect conflicts in a batch of claims
    pub fn detect_conflicts_batch(
        &self,
        new_claims: &[EpistemicClaim],
        existing_claims: &[EpistemicClaim],
    ) -> Vec<ConflictRecord> {
        let mut conflicts = Vec::new();

        for new_claim in new_claims {
            for existing in existing_claims {
                if let Some(conflict) = super::conflict::detect_conflict(new_claim, existing) {
                    conflicts.push(conflict);
                }
            }
        }

        conflicts
    }

    /// Record paradigm shift result
    pub fn record_shift(
        &mut self,
        result: ShiftResult,
        old_hash: String,
        new_hash: Option<String>,
        improvement: Option<f64>,
    ) {
        let record = ShiftRecord {
            timestamp: chrono::Utc::now(),
            result,
            old_framework_hash: old_hash,
            new_framework_hash: new_hash,
            improvement_pct: improvement,
        };

        self.state.shift_history.push(record);
        self.state.anomaly_counter = 0;
        self.state.ticks_since_last_shift = 0;
        self.state.mode = EvolutionMode::Incremental;
    }

    /// Check if paradigm shift should be accepted
    pub fn should_accept_shift(&self, improvement_pct: f64) -> bool {
        improvement_pct >= self.config.min_improvement_pct
    }

    /// Get conflict density for a set of claims
    pub fn calculate_conflict_density(
        &self,
        claims: &[EpistemicClaim],
        conflicts: &[ConflictRecord],
    ) -> f64 {
        if claims.len() < 2 {
            return 0.0;
        }

        let max_conflicts = (claims.len() * (claims.len() - 1)) / 2;
        conflicts.len() as f64 / max_conflicts as f64
    }
}

/// Calculate diversity entropy for federation health
pub fn calculate_diversity_entropy(sources: &[String]) -> f64 {
    use std::collections::HashMap;

    if sources.is_empty() {
        return 0.0;
    }

    let mut counts: HashMap<&String, usize> = HashMap::new();
    for source in sources {
        *counts.entry(source).or_insert(0) += 1;
    }

    let total = sources.len() as f64;
    let mut entropy = 0.0;

    for count in counts.values() {
        let p = *count as f64 / total;
        entropy -= p * p.log2();
    }

    // Normalize by max possible entropy
    let max_entropy = (counts.len() as f64).log2();
    if max_entropy > 0.0 {
        entropy / max_entropy
    } else {
        0.0
    }
}

/// Calculate Gini coefficient for centralization measurement
pub fn calculate_gini(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = sorted.len() as f64;
    let sum: f64 = sorted.iter().sum();

    if sum == 0.0 {
        return 0.0;
    }

    let mut cumsum = 0.0;
    let mut lorenz_sum = 0.0;

    for v in sorted.iter() {
        cumsum += v;
        lorenz_sum += cumsum / sum;
    }

    (n + 1.0 - 2.0 * lorenz_sum) / n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evolution_engine_tick() {
        let mut engine = EvolutionEngine::new(EvolutionConfig::default());

        // Normal signals
        let normal_signals = AnomalySignals {
            prediction_error_streak: 0,
            conflict_density_pct: 0.1,
            cache_hit_rate_trend: 0.0,
        };

        engine.tick(normal_signals);
        assert_eq!(engine.state().mode, EvolutionMode::Incremental);

        // Anomalous signals
        let anomaly_signals = AnomalySignals {
            prediction_error_streak: 10,
            conflict_density_pct: 0.5,
            cache_hit_rate_trend: -0.3,
        };

        for _ in 0..100 {
            engine.tick(anomaly_signals.clone());
        }

        assert_eq!(engine.state().mode, EvolutionMode::ParadigmShift);
    }

    #[test]
    fn test_diversity_entropy() {
        let sources = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        let entropy = calculate_diversity_entropy(&sources);
        assert!(entropy > 0.0 && entropy <= 1.0);

        let uniform = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let max_entropy = calculate_diversity_entropy(&uniform);
        assert!(max_entropy > entropy);
    }

    #[test]
    fn test_gini_coefficient() {
        // Perfect equality
        let equal = vec![1.0, 1.0, 1.0];
        assert!(calculate_gini(&equal) < 0.1);

        // High inequality
        let unequal = vec![0.1, 0.1, 10.0];
        assert!(calculate_gini(&unequal) > 0.5);
    }
}
