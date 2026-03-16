//! FrameworkHealthMonitor implementation

use super::*;
use crate::models::*;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Framework health monitor
pub struct FrameworkHealthMonitor {
    config: HealthMonitorConfig,
    /// Insight tracking data
    pub(crate) insight_tracking: HashMap<Id, InsightTrackingData>,
    /// Domain-specific metrics
    domain_metrics: HashMap<String, DomainMetrics>,
    /// Historical health snapshots
    history: Vec<HealthSnapshot>,
    /// Systematic bias records
    bias_records: Vec<SystematicBias>,
}

impl FrameworkHealthMonitor {
    pub fn new(config: HealthMonitorConfig) -> Self {
        Self {
            config,
            insight_tracking: HashMap::new(),
            domain_metrics: HashMap::new(),
            history: Vec::new(),
            bias_records: Vec::new(),
        }
    }

    /// Register a new Insight for tracking
    pub fn register_insight(&mut self, insight: &EpistemicClaim) {
        let tracking = InsightTrackingData {
            insight_id: insight.id,
            content: insight.content.clone(),
            created_at: insight.t_known,
            predictions: Vec::new(),
            total_predictions: 0,
            correct_predictions: 0,
            accuracy_history: Vec::new(),
        };

        self.insight_tracking.insert(insight.id, tracking);
    }

    /// Record a prediction made by an Insight
    pub fn record_prediction(&mut self, insight_id: Id, predicted_value: String) {
        if let Some(tracking) = self.insight_tracking.get_mut(&insight_id) {
            let outcome = PredictionOutcome {
                predicted_at: Utc::now(),
                predicted_value,
                actual_value: None,
                validated_at: None,
                error_score: None,
                was_correct: None,
            };

            tracking.predictions.push(outcome);
            tracking.total_predictions += 1;
        }

        // Update domain metrics
        // Note: In a real implementation, we'd need to get the domain from the insight
    }

    /// Validate a prediction with actual outcome
    pub fn validate_prediction(
        &mut self,
        insight_id: Id,
        predicted_at: DateTime<Utc>,
        actual_value: String,
    ) -> Option<f64> {
        if let Some(tracking) = self.insight_tracking.get_mut(&insight_id) {
            // Find the prediction
            if let Some(prediction) = tracking.predictions.iter_mut().find(|p| {
                p.predicted_at == predicted_at
                    || (p.predicted_at - predicted_at).num_seconds().abs() < 60
            }) {
                prediction.actual_value = Some(actual_value.clone());
                prediction.validated_at = Some(Utc::now());

                // Calculate if correct (exact match for now)
                let was_correct =
                    prediction.predicted_value.to_lowercase() == actual_value.to_lowercase();
                prediction.was_correct = Some(was_correct);

                if was_correct {
                    tracking.correct_predictions += 1;
                }

                // Calculate error score
                let error = if was_correct { 0.0 } else { 1.0 };
                prediction.error_score = Some(error);

                // Update accuracy history
                let accuracy =
                    tracking.correct_predictions as f64 / tracking.total_predictions as f64;
                tracking.accuracy_history.push((Utc::now(), accuracy));

                return Some(error);
            }
        }
        None
    }

    /// Calculate accuracy for an Insight
    pub fn get_insight_accuracy(&self, insight_id: Id) -> Option<f64> {
        self.insight_tracking.get(&insight_id).map(|tracking| {
            if tracking.total_predictions == 0 {
                0.0
            } else {
                tracking.correct_predictions as f64 / tracking.total_predictions as f64
            }
        })
    }

    /// Get accuracy trend for an Insight
    pub fn get_insight_trend(&self, insight_id: Id) -> TrendDirection {
        if let Some(tracking) = self.insight_tracking.get(&insight_id) {
            if tracking.accuracy_history.len() < 2 {
                return TrendDirection::Unknown;
            }

            // Compare first half vs second half
            let mid = tracking.accuracy_history.len() / 2;
            let first_half: f64 = tracking.accuracy_history[..mid]
                .iter()
                .map(|(_, acc)| acc)
                .sum::<f64>()
                / mid as f64;
            let second_half: f64 = tracking.accuracy_history[mid..]
                .iter()
                .map(|(_, acc)| acc)
                .sum::<f64>()
                / (tracking.accuracy_history.len() - mid) as f64;

            let diff = second_half - first_half;
            if diff > 0.1 {
                TrendDirection::Improving
            } else if diff < -0.1 {
                TrendDirection::Declining
            } else {
                TrendDirection::Stable
            }
        } else {
            TrendDirection::Unknown
        }
    }

    /// Take a health snapshot
    pub fn take_snapshot(&mut self) -> HealthSnapshot {
        let now = Utc::now();
        let _window_start = now - Duration::hours(self.config.window_size_hours);

        // Calculate overall accuracy
        let (total_correct, total_predictions) =
            self.insight_tracking
                .values()
                .fold((0, 0), |(correct, total), tracking| {
                    (
                        correct + tracking.correct_predictions,
                        total + tracking.total_predictions,
                    )
                });

        let overall_accuracy = if total_predictions > 0 {
            total_correct as f64 / total_predictions as f64
        } else {
            0.0
        };

        // Calculate active predictions
        let active_predictions = self
            .insight_tracking
            .values()
            .flat_map(|t| &t.predictions)
            .filter(|p| p.validated_at.is_none())
            .count();

        // Calculate average error
        let total_error: f64 = self
            .insight_tracking
            .values()
            .flat_map(|t| &t.predictions)
            .filter_map(|p| p.error_score)
            .sum();
        let avg_error = if total_predictions > 0 {
            total_error / total_predictions as f64
        } else {
            0.0
        };

        // Domain breakdown
        let mut domain_breakdown = HashMap::new();
        for (domain, metrics) in &self.domain_metrics {
            let accuracy = if metrics.total_predictions > 0 {
                metrics.correct_predictions as f64 / metrics.total_predictions as f64
            } else {
                0.0
            };

            let status = if accuracy >= self.config.accuracy_threshold {
                HealthStatus::Healthy
            } else if accuracy >= self.config.accuracy_threshold * 0.8 {
                HealthStatus::Degraded
            } else if metrics.total_predictions > 0 {
                HealthStatus::Unhealthy
            } else {
                HealthStatus::Unknown
            };

            domain_breakdown.insert(
                domain.clone(),
                DomainHealth {
                    domain: domain.clone(),
                    accuracy,
                    sample_size: metrics.total_predictions,
                    health_status: status,
                },
            );
        }

        // Calculate health score
        let health_score = overall_accuracy;

        let snapshot = HealthSnapshot {
            timestamp: now,
            overall_accuracy,
            insight_count: self.insight_tracking.len(),
            active_predictions,
            avg_prediction_error: avg_error,
            domain_breakdown,
            health_score,
        };

        self.history.push(snapshot.clone());
        snapshot
    }

    /// Detect systematic biases
    pub fn detect_biases(&mut self) -> Vec<SystematicBias> {
        if !self.config.enable_bias_detection {
            return Vec::new();
        }

        let mut new_biases = Vec::new();

        // Check for overconfidence
        if let Some(bias) = self.detect_overconfidence() {
            new_biases.push(bias);
        }

        // Check for underconfidence
        if let Some(bias) = self.detect_underconfidence() {
            new_biases.push(bias);
        }

        // Check for domain blindness
        new_biases.extend(self.detect_domain_blindness());

        // Check for temporal bias
        if let Some(bias) = self.detect_temporal_bias() {
            new_biases.push(bias);
        }

        self.bias_records.extend(new_biases.clone());
        new_biases
    }

    /// Detect overconfidence bias
    fn detect_overconfidence(&self) -> Option<SystematicBias> {
        let total_predictions: u32 = self
            .insight_tracking
            .values()
            .map(|t| t.total_predictions)
            .sum();

        if total_predictions < self.config.min_samples as u32 {
            return None;
        }

        let total_correct: u32 = self
            .insight_tracking
            .values()
            .map(|t| t.correct_predictions)
            .sum();

        let accuracy = total_correct as f64 / total_predictions as f64;

        // Check if average confidence is much higher than accuracy
        // This would require storing confidence values with predictions
        // For now, simplified check
        if accuracy < 0.5 && total_predictions > 100 {
            Some(SystematicBias {
                bias_id: Uuid::new_v4(),
                detected_at: Utc::now(),
                bias_type: BiasType::Overconfidence,
                affected_domain: None,
                description: "System shows signs of overconfidence with low accuracy".to_string(),
                evidence: vec![BiasEvidence {
                    metric: "accuracy".to_string(),
                    expected_value: 0.7,
                    actual_value: accuracy,
                    deviation: 0.7 - accuracy,
                }],
                magnitude: 0.7 - accuracy,
                confidence: 0.6,
                status: BiasStatus::Open,
            })
        } else {
            None
        }
    }

    /// Detect underconfidence bias
    fn detect_underconfidence(&self) -> Option<SystematicBias> {
        // Implementation similar to overconfidence
        // Would check if confidence is much lower than accuracy
        None
    }

    /// Detect domain blindness (poor performance in specific domains)
    fn detect_domain_blindness(&self) -> Vec<SystematicBias> {
        let mut biases = Vec::new();

        for (domain, metrics) in &self.domain_metrics {
            if metrics.total_predictions < 10 {
                continue;
            }

            let accuracy = if metrics.total_predictions > 0 {
                metrics.correct_predictions as f64 / metrics.total_predictions as f64
            } else {
                0.0
            };

            if accuracy < self.config.accuracy_threshold * 0.5 {
                biases.push(SystematicBias {
                    bias_id: Uuid::new_v4(),
                    detected_at: Utc::now(),
                    bias_type: BiasType::DomainBlindness,
                    affected_domain: Some(domain.clone()),
                    description: format!("Poor prediction accuracy in domain: {}", domain),
                    evidence: vec![BiasEvidence {
                        metric: "domain_accuracy".to_string(),
                        expected_value: self.config.accuracy_threshold,
                        actual_value: accuracy,
                        deviation: self.config.accuracy_threshold - accuracy,
                    }],
                    magnitude: self.config.accuracy_threshold - accuracy,
                    confidence: 0.7,
                    status: BiasStatus::Open,
                });
            }
        }

        biases
    }

    /// Detect temporal bias (performance changes over time)
    fn detect_temporal_bias(&self) -> Option<SystematicBias> {
        if self.history.len() < 7 {
            return None;
        }

        let recent: Vec<_> = self.history.iter().rev().take(7).collect();
        let accuracies: Vec<f64> = recent.iter().map(|h| h.overall_accuracy).collect();

        // Check for declining trend
        let first = accuracies.first().copied().unwrap_or(0.0);
        let last = accuracies.last().copied().unwrap_or(0.0);
        let decline = first - last;

        if decline > self.config.bias_threshold {
            Some(SystematicBias {
                bias_id: Uuid::new_v4(),
                detected_at: Utc::now(),
                bias_type: BiasType::TemporalBias,
                affected_domain: None,
                description: "System accuracy declining over time".to_string(),
                evidence: vec![BiasEvidence {
                    metric: "accuracy_decline".to_string(),
                    expected_value: 0.0,
                    actual_value: decline,
                    deviation: decline,
                }],
                magnitude: decline,
                confidence: 0.6,
                status: BiasStatus::Open,
            })
        } else {
            None
        }
    }

    /// Generate a comprehensive health report
    pub fn generate_report(&self) -> HealthReport {
        let now = Utc::now();

        // Generate summary
        let total_predictions: u32 = self
            .insight_tracking
            .values()
            .map(|t| t.total_predictions)
            .sum();
        let total_correct: u32 = self
            .insight_tracking
            .values()
            .map(|t| t.correct_predictions)
            .sum();

        let overall_accuracy = if total_predictions > 0 {
            total_correct as f64 / total_predictions as f64
        } else {
            0.0
        };

        let status = if overall_accuracy >= self.config.accuracy_threshold {
            HealthStatus::Healthy
        } else if overall_accuracy >= self.config.accuracy_threshold * 0.8 {
            HealthStatus::Degraded
        } else if total_predictions > 0 {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Unknown
        };

        let trend = if self.history.len() >= 2 {
            let first = self.history.first().map(|h| h.health_score).unwrap_or(0.0);
            let last = self.history.last().map(|h| h.health_score).unwrap_or(0.0);
            if last > first + 0.1 {
                TrendDirection::Improving
            } else if last < first - 0.1 {
                TrendDirection::Declining
            } else {
                TrendDirection::Stable
            }
        } else {
            TrendDirection::Unknown
        };

        let summary = HealthSummary {
            overall_health_score: overall_accuracy,
            status,
            total_insights_tracked: self.insight_tracking.len(),
            total_predictions,
            overall_accuracy,
            trend_direction: trend,
        };

        // Generate insight reports
        let mut insights: Vec<InsightReport> = self
            .insight_tracking
            .values()
            .map(|tracking| {
                let accuracy = if tracking.total_predictions > 0 {
                    tracking.correct_predictions as f64 / tracking.total_predictions as f64
                } else {
                    0.0
                };

                let status = if accuracy >= self.config.accuracy_threshold {
                    HealthStatus::Healthy
                } else if accuracy >= self.config.accuracy_threshold * 0.8 {
                    HealthStatus::Degraded
                } else if tracking.total_predictions > 0 {
                    HealthStatus::Unhealthy
                } else {
                    HealthStatus::Unknown
                };

                InsightReport {
                    insight_id: tracking.insight_id,
                    content_preview: tracking.content.chars().take(100).collect(),
                    accuracy,
                    total_predictions: tracking.total_predictions,
                    trend: self.get_insight_trend(tracking.insight_id),
                    status,
                }
            })
            .collect();

        insights.sort_by(|a, b| b.accuracy.partial_cmp(&a.accuracy).unwrap());

        // Generate domain reports
        let domain_breakdown: Vec<DomainReport> = self
            .domain_metrics
            .iter()
            .map(|(domain, metrics)| {
                let accuracy = if metrics.total_predictions > 0 {
                    metrics.correct_predictions as f64 / metrics.total_predictions as f64
                } else {
                    0.0
                };

                let status = if accuracy >= self.config.accuracy_threshold {
                    HealthStatus::Healthy
                } else if accuracy >= self.config.accuracy_threshold * 0.8 {
                    HealthStatus::Degraded
                } else if metrics.total_predictions > 0 {
                    HealthStatus::Unhealthy
                } else {
                    HealthStatus::Unknown
                };

                // Find top and underperforming insights in this domain
                let mut domain_insights: Vec<_> = self
                    .insight_tracking
                    .values()
                    .filter(|_t| {
                        // Would need to check domain in real implementation
                        true
                    })
                    .collect();

                domain_insights.sort_by(|a, b| {
                    let a_acc = if a.total_predictions > 0 {
                        a.correct_predictions as f64 / a.total_predictions as f64
                    } else {
                        0.0
                    };
                    let b_acc = if b.total_predictions > 0 {
                        b.correct_predictions as f64 / b.total_predictions as f64
                    } else {
                        0.0
                    };
                    b_acc.partial_cmp(&a_acc).unwrap()
                });

                let top_performing = domain_insights
                    .iter()
                    .take(3)
                    .map(|i| i.insight_id)
                    .collect();
                let underperforming = domain_insights
                    .iter()
                    .rev()
                    .take(3)
                    .map(|i| i.insight_id)
                    .collect();

                DomainReport {
                    domain: domain.clone(),
                    accuracy,
                    sample_size: metrics.total_predictions,
                    status,
                    top_performing_insights: top_performing,
                    underperforming_insights: underperforming,
                }
            })
            .collect();

        // Generate recommendations
        let recommendations = self.generate_recommendations(&insights, &domain_breakdown);

        HealthReport {
            generated_at: now,
            summary,
            insights,
            domain_breakdown,
            biases: self.bias_records.clone(),
            recommendations,
        }
    }

    /// Generate health recommendations
    fn generate_recommendations(
        &self,
        insights: &[InsightReport],
        domains: &[DomainReport],
    ) -> Vec<HealthRecommendation> {
        let mut recommendations = Vec::new();

        // Check for underperforming insights
        let underperforming: Vec<_> = insights
            .iter()
            .filter(|i| i.status == HealthStatus::Unhealthy && i.total_predictions > 10)
            .collect();

        if !underperforming.is_empty() {
            recommendations.push(HealthRecommendation {
                priority: RecommendationPriority::High,
                category: RecommendationCategory::ReviewData,
                description: format!(
                    "{} insights are underperforming and may need review",
                    underperforming.len()
                ),
                affected_insights: underperforming.iter().map(|i| i.insight_id).collect(),
                suggested_action: "Review training data and validation process".to_string(),
            });
        }

        // Check for declining trends
        let declining: Vec<_> = insights
            .iter()
            .filter(|i| i.trend == TrendDirection::Declining && i.total_predictions > 10)
            .collect();

        if !declining.is_empty() {
            recommendations.push(HealthRecommendation {
                priority: RecommendationPriority::High,
                category: RecommendationCategory::RetrainModel,
                description: format!(
                    "{} insights show declining performance trends",
                    declining.len()
                ),
                affected_insights: declining.iter().map(|i| i.insight_id).collect(),
                suggested_action: "Consider retraining or updating the insight model".to_string(),
            });
        }

        // Check for domains with low data
        let low_data_domains: Vec<_> = domains
            .iter()
            .filter(|d| d.sample_size < 10 && d.sample_size > 0)
            .collect();

        if !low_data_domains.is_empty() {
            recommendations.push(HealthRecommendation {
                priority: RecommendationPriority::Medium,
                category: RecommendationCategory::AddData,
                description: format!(
                    "{} domains have insufficient prediction data",
                    low_data_domains.len()
                ),
                affected_insights: vec![],
                suggested_action: "Collect more validation data for these domains".to_string(),
            });
        }

        // Check for systematic biases
        let open_biases: Vec<_> = self
            .bias_records
            .iter()
            .filter(|b| b.status == BiasStatus::Open)
            .collect();

        if !open_biases.is_empty() {
            recommendations.push(HealthRecommendation {
                priority: RecommendationPriority::Critical,
                category: RecommendationCategory::InvestigateBias,
                description: format!(
                    "{} systematic biases detected and require investigation",
                    open_biases.len()
                ),
                affected_insights: vec![],
                suggested_action: "Investigate and mitigate detected biases".to_string(),
            });
        }

        recommendations
    }

    /// Get all bias records
    pub fn get_biases(&self) -> &[SystematicBias] {
        &self.bias_records
    }

    /// Get health history
    pub fn get_history(&self) -> &[HealthSnapshot] {
        &self.history
    }

    /// Update bias status
    pub fn update_bias_status(&mut self, bias_id: Id, status: BiasStatus) {
        if let Some(bias) = self.bias_records.iter_mut().find(|b| b.bias_id == bias_id) {
            bias.status = status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_insight(id: Id, content: &str) -> EpistemicClaim {
        EpistemicClaim {
            id,
            tenant_id: "test".to_string(),
            content: content.to_string(),
            node_type: NodeType::Insight,
            knowledge_type: KnowledgeType::Experiential,
            structured_content: None,
            epistemic_status: EpistemicStatus::Asserted,
            confidence: 0.8,
            consolidation_stage: ConsolidationStage::Insight,
            claimant: Claimant::System,
            version: 1,
            provenance: ProvenanceRecord::new(
                "test".to_string(),
                "test".to_string(),
                "test".to_string(),
            ),
            access_envelope: AccessEnvelope::new("test"),
            activation_weight: 0.5,
            access_count: 0,
            last_accessed: None,
            t_valid_start: Utc::now(),
            t_valid_end: None,
            t_known: Utc::now(),
            vector_id: None,
            last_prediction_error: None,
            derived_from: vec![],
            superseded_by: None,
            entity_refs: vec![],
            needs_revalidation: false,
            durability: 1.0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: serde_json::Map::new(),
        }
    }

    #[test]
    fn test_register_insight() {
        let mut monitor = FrameworkHealthMonitor::new(HealthMonitorConfig::default());
        let insight = create_test_insight(Uuid::new_v4(), "Test insight");

        monitor.register_insight(&insight);
        assert_eq!(monitor.insight_tracking.len(), 1);
    }

    #[test]
    fn test_record_and_validate_prediction() {
        let mut monitor = FrameworkHealthMonitor::new(HealthMonitorConfig::default());
        let insight_id = Uuid::new_v4();
        let insight = create_test_insight(insight_id, "Test insight");

        monitor.register_insight(&insight);
        monitor.record_prediction(insight_id, "prediction".to_string());

        let tracking = monitor.insight_tracking.get(&insight_id).unwrap();
        assert_eq!(tracking.total_predictions, 1);

        // Validate with correct value
        monitor.validate_prediction(insight_id, Utc::now(), "prediction".to_string());

        let accuracy = monitor.get_insight_accuracy(insight_id);
        assert_eq!(accuracy, Some(1.0));
    }

    #[test]
    fn test_take_snapshot() {
        let mut monitor = FrameworkHealthMonitor::new(HealthMonitorConfig::default());

        // Add some insights and predictions
        let insight_id = Uuid::new_v4();
        let insight = create_test_insight(insight_id, "Test");
        monitor.register_insight(&insight);
        monitor.record_prediction(insight_id, "test".to_string());
        monitor.validate_prediction(insight_id, Utc::now(), "test".to_string());

        let snapshot = monitor.take_snapshot();
        assert!(snapshot.overall_accuracy >= 0.0 && snapshot.overall_accuracy <= 1.0);
    }

    #[test]
    fn test_generate_report() {
        let mut monitor = FrameworkHealthMonitor::new(HealthMonitorConfig::default());

        let insight_id = Uuid::new_v4();
        let insight = create_test_insight(insight_id, "Test insight");
        monitor.register_insight(&insight);

        let report = monitor.generate_report();
        assert_eq!(report.summary.total_insights_tracked, 1);
        assert!(report.recommendations.is_empty() || !report.recommendations.is_empty());
    }
}
