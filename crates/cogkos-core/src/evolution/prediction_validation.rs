//! Prediction validation and feedback loop
//! Phase 3: Validate predictions and feed errors back to evolution engine

use crate::models::{EpistemicClaim, ConsolidationStage, PredictionOutcome};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Prediction validation record
#[derive(Debug, Clone)]
pub struct PredictionValidation {
    pub id: uuid::Uuid,
    pub claim_id: uuid::Uuid,
    pub predicted_outcome: String,
    pub actual_outcome: Option<String>,
    pub predicted_probability: f64,
    pub actual_result: Option<bool>,
    pub prediction_error: f64,
    pub validation_timestamp: Option<DateTime<Utc>>,
    pub feedback_source: String,
    pub confidence_adjustment: f64,
}

/// Prediction validator for tracking and validating predictions
pub struct PredictionValidator {
    validations: HashMap<uuid::Uuid, PredictionValidation>,
    config: ValidationConfig,
}

/// Configuration for prediction validation
#[derive(Clone, Debug)]
pub struct ValidationConfig {
    /// Maximum age for pending validations (days)
    pub max_pending_days: i64,
    /// Confidence penalty for incorrect predictions
    pub incorrect_penalty: f64,
    /// Confidence boost for correct predictions
    pub correct_boost: f64,
    /// Minimum confidence adjustment
    pub min_adjustment: f64,
    /// Maximum confidence adjustment
    pub max_adjustment: f64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_pending_days: 30,
            incorrect_penalty: 0.15,
            correct_boost: 0.10,
            min_adjustment: 0.01,
            max_adjustment: 0.30,
        }
    }
}

impl PredictionValidator {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            validations: HashMap::new(),
            config,
        }
    }

    /// Register a new prediction for validation
    pub fn register_prediction(
        &mut self,
        claim_id: uuid::Uuid,
        predicted_outcome: String,
        predicted_probability: f64,
        feedback_source: String,
    ) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();

        let validation = PredictionValidation {
            id,
            claim_id,
            predicted_outcome,
            actual_outcome: None,
            predicted_probability,
            actual_result: None,
            prediction_error: 0.0,
            validation_timestamp: None,
            feedback_source,
            confidence_adjustment: 0.0,
        };

        self.validations.insert(id, validation);
        id
    }

    /// Submit validation feedback for a prediction
    pub fn submit_validation(
        &mut self,
        validation_id: uuid::Uuid,
        actual_outcome: String,
        actual_result: bool,
    ) -> Option<PredictionOutcome> {
        let validation = self.validations.get_mut(&validation_id)?;

        validation.actual_outcome = Some(actual_outcome);
        validation.actual_result = Some(actual_result);
        validation.validation_timestamp = Some(Utc::now());

        // Calculate prediction error
        let predicted = validation.predicted_probability;
        let actual = if actual_result { 1.0 } else { 0.0 };
        validation.prediction_error = (predicted - actual).abs();

        // Calculate confidence adjustment
        validation.confidence_adjustment = self.calculate_confidence_adjustment(
            actual_result,
            validation.prediction_error,
        );

        Some(self.create_outcome(validation))
    }

    /// Submit validation by claim ID (for when validation_id is unknown)
    pub fn submit_validation_by_claim(
        &mut self,
        claim_id: uuid::Uuid,
        actual_outcome: String,
        actual_result: bool,
    ) -> Vec<PredictionOutcome> {
        let mut outcomes = Vec::new();

        let validation_ids: Vec<uuid::Uuid> = self.validations
            .values()
            .filter(|v| v.claim_id == claim_id && v.actual_result.is_none())
            .map(|v| v.id)
            .collect();

        for id in validation_ids {
            if let Some(outcome) = self.submit_validation(id, actual_outcome.clone(), actual_result) {
                outcomes.push(outcome);
            }
        }

        outcomes
    }

    /// Calculate confidence adjustment based on validation result
    fn calculate_confidence_adjustment(&self, correct: bool, error: f64) -> f64 {
        let adjustment = if correct {
            self.config.correct_boost * (1.0 - error)
        } else {
            -self.config.incorrect_penalty * error
        };

        adjustment.clamp(-self.config.max_adjustment, self.config.max_adjustment)
            .abs()
            .max(self.config.min_adjustment)
            * if adjustment < 0.0 { -1.0 } else { 1.0 }
    }

    /// Create prediction outcome from validation
    fn create_outcome(&self, validation: &PredictionValidation) -> PredictionOutcome {
        PredictionOutcome {
            validation_id: validation.id,
            claim_id: validation.claim_id,
            predicted: validation.predicted_probability,
            actual: validation.actual_result.unwrap_or(false),
            error: validation.prediction_error,
            confidence_delta: validation.confidence_adjustment,
        }
    }

    /// Get validation statistics
    pub fn get_statistics(&self) -> ValidationStatistics {
        let all_validations: Vec<&PredictionValidation> = self.validations.values().collect();

        let total = all_validations.len();
        let validated: Vec<_> = all_validations.iter()
            .filter(|v| v.actual_result.is_some())
            .copied()
            .collect();

        let pending = total - validated.len();

        if validated.is_empty() {
            return ValidationStatistics {
                total,
                validated: 0,
                pending,
                correct: 0,
                incorrect: 0,
                accuracy: 0.0,
                average_error: 0.0,
                calibration_score: 0.0,
            };
        }

        let correct = validated.iter()
            .filter(|v| v.actual_result == Some(true))
            .count();
        let incorrect = validated.len() - correct;

        let accuracy = correct as f64 / validated.len() as f64;

        let avg_error = validated.iter()
            .map(|v| v.prediction_error)
            .sum::<f64>() / validated.len() as f64;

        // Calibration: how well do predicted probabilities match actual outcomes
        let calibration = self.calculate_calibration(&validated);

        ValidationStatistics {
            total,
            validated: validated.len(),
            pending,
            correct,
            incorrect,
            accuracy,
            average_error: avg_error,
            calibration_score: calibration,
        }
    }

    /// Calculate calibration score (Brier score component)
    fn calculate_calibration(&self, validations: &[&PredictionValidation]) -> f64 {
        // Group predictions by probability bins
        let bins: Vec<Vec<f64>> = vec![Vec::new(); 10]; // 10 bins: 0-0.1, 0.1-0.2, etc.

        for v in validations {
            let bin_idx = (v.predicted_probability * 10.0).min(9.0) as usize;
            let actual = if v.actual_result == Some(true) { 1.0 } else { 0.0 };
            // This is a simplified version - in reality we'd store bin assignments
        }

        // Simplified calibration: 1 - average error (higher is better calibrated)
        1.0 - validations.iter().map(|v| v.prediction_error).sum::<f64>() / validations.len() as f64
    }

    /// Clean up old pending validations
    pub fn cleanup_old_validations(&mut self) -> usize {
        let cutoff = Utc::now() - chrono::Duration::days(self.config.max_pending_days);

        let to_remove: Vec<uuid::Uuid> = self.validations
            .values()
            .filter(|v| {
                v.actual_result.is_none() &&
                Utc::now().signed_duration_since(
                    v.validation_timestamp.unwrap_or(Utc::now())
                ).num_days() > self.config.max_pending_days
            })
            .map(|v| v.id)
            .collect();

        for id in &to_remove {
            self.validations.remove(id);
        }

        to_remove.len()
    }

    /// Get all pending validations
    pub fn get_pending_validations(&self) -> Vec<&PredictionValidation> {
        self.validations
            .values()
            .filter(|v| v.actual_result.is_none())
            .collect()
    }

    /// Get validation by ID
    pub fn get_validation(&self, id: uuid::Uuid) -> Option<&PredictionValidation> {
        self.validations.get(&id)
    }
}

/// Statistics for validation tracking
#[derive(Debug, Clone)]
pub struct ValidationStatistics {
    pub total: usize,
    pub validated: usize,
    pub pending: usize,
    pub correct: usize,
    pub incorrect: usize,
    pub accuracy: f64,
    pub average_error: f64,
    pub calibration_score: f64,
}

/// Apply prediction outcomes to update claim confidence
pub fn apply_prediction_outcomes(
    claim: &mut EpistemicClaim,
    outcomes: &[PredictionOutcome],
) -> f64 {
    if outcomes.is_empty() {
        return claim.confidence;
    }

    // Calculate aggregate adjustment
    let total_adjustment: f64 = outcomes.iter()
        .map(|o| o.confidence_delta)
        .sum();

    let avg_adjustment = total_adjustment / outcomes.len() as f64;

    // Apply bounded adjustment
    let new_confidence = (claim.confidence + avg_adjustment).clamp(0.1, 0.99);

    // Update claim
    claim.confidence = new_confidence;
    claim.prediction_error = outcomes.iter()
        .map(|o| o.error)
        .sum::<f64>() / outcomes.len() as f64;

    // Update consolidation stage based on accuracy
    if outcomes.len() >= 3 {
        let accuracy = outcomes.iter()
            .filter(|o| o.actual)
            .count() as f64 / outcomes.len() as f64;

        if accuracy > 0.7 && claim.consolidation_stage == ConsolidationStage::FastTrack {
            claim.consolidation_stage = ConsolidationStage::Consolidated;
        }
    }

    new_confidence
}

/// Batch validation result
#[derive(Debug, Clone)]
pub struct BatchValidationResult {
    pub processed: usize,
    pub updated_claims: Vec<uuid::Uuid>,
    pub statistics: ValidationStatistics,
}

/// Process batch validation feedback
pub fn process_batch_validation(
    validator: &mut PredictionValidator,
    feedback: Vec<(uuid::Uuid, String, bool)>, // (claim_id, actual_outcome, actual_result)
) -> BatchValidationResult {
    let mut updated_claims: Vec<uuid::Uuid> = Vec::new();

    for (claim_id, outcome, result) in feedback {
        let outcomes = validator.submit_validation_by_claim(claim_id, outcome, result);
        if !outcomes.is_empty() {
            updated_claims.push(claim_id);
        }
    }

    BatchValidationResult {
        processed: feedback.len(),
        updated_claims,
        statistics: validator.get_statistics(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    #[test]
    fn test_register_and_validate_prediction() {
        let mut validator = PredictionValidator::new(ValidationConfig::default());

        let claim_id = uuid::Uuid::new_v4();
        let validation_id = validator.register_prediction(
            claim_id,
            "Price will increase".to_string(),
            0.75,
            "agent_feedback".to_string(),
        );

        let outcome = validator.submit_validation(
            validation_id,
            "Price increased by 10%".to_string(),
            true,
        );

        assert!(outcome.is_some());
        let outcome = outcome.unwrap();
        assert!(outcome.actual);
        assert!(outcome.error < 0.3); // 0.75 predicted, 1.0 actual
        assert!(outcome.confidence_delta > 0.0);
    }

    #[test]
    fn test_incorrect_prediction() {
        let mut validator = PredictionValidator::new(ValidationConfig::default());

        let claim_id = uuid::Uuid::new_v4();
        let validation_id = validator.register_prediction(
            claim_id,
            "Price will increase".to_string(),
            0.80,
            "agent_feedback".to_string(),
        );

        let outcome = validator.submit_validation(
            validation_id,
            "Price decreased".to_string(),
            false,
        );

        assert!(outcome.is_some());
        let outcome = outcome.unwrap();
        assert!(!outcome.actual);
        assert!(outcome.error > 0.8);
        assert!(outcome.confidence_delta < 0.0);
    }

    #[test]
    fn test_statistics() {
        let mut validator = PredictionValidator::new(ValidationConfig::default());

        // Register 3 predictions
        let id1 = validator.register_prediction(uuid::Uuid::new_v4(), "A".to_string(), 0.8, "test".to_string());
        let id2 = validator.register_prediction(uuid::Uuid::new_v4(), "B".to_string(), 0.7, "test".to_string());
        let id3 = validator.register_prediction(uuid::Uuid::new_v4(), "C".to_string(), 0.6, "test".to_string());

        // Validate 2: 1 correct, 1 incorrect
        validator.submit_validation(id1, "".to_string(), true);
        validator.submit_validation(id2, "".to_string(), false);

        let stats = validator.get_statistics();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.validated, 2);
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.correct, 1);
        assert_eq!(stats.incorrect, 1);
        assert!((stats.accuracy - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_apply_prediction_outcomes() {
        let mut claim = EpistemicClaim::new(
            "test-tenant".to_string(),
            "Test claim".to_string(),
            NodeType::Prediction,
            Claimant::System,
            AccessEnvelope::new("test-tenant"),
            ProvenanceRecord::new("test", "test"),
        );
        claim.confidence = 0.7;

        let outcomes = vec![
            PredictionOutcome {
                validation_id: uuid::Uuid::new_v4(),
                claim_id: claim.id,
                predicted: 0.8,
                actual: true,
                error: 0.2,
                confidence_delta: 0.08,
            },
        ];

        let new_confidence = apply_prediction_outcomes(&mut claim, &outcomes);
        assert!(new_confidence > 0.7);
        assert_eq!(claim.prediction_error, 0.2);
    }
}
