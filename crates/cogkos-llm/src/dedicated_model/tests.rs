use super::*;
use chrono::Utc;
use std::collections::HashMap;

fn create_test_samples(count: usize, class: &str) -> Vec<TrainingSample> {
    (0..count)
        .map(|i| TrainingSample {
            id: format!("sample_{}", i),
            features: vec![i as f64 * 0.1, i as f64 * 0.2],
            class_label: class.to_string(),
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        })
        .collect()
}

#[test]
fn test_data_sufficiency_analyzer() {
    let analyzer = DataSufficiencyAnalyzer::new();

    // Test with sufficient data
    let samples = create_test_samples(150, "class_a");
    let result = analyzer.analyze(&samples);

    assert!(result.sample_count >= 100);
    assert!(result.diversity_score > 0.0);
}

#[test]
fn test_insufficient_data() {
    let analyzer = DataSufficiencyAnalyzer::new();

    let samples = create_test_samples(50, "class_a");
    let result = analyzer.analyze(&samples);

    assert!(!result.is_sufficient);
    assert!(!result.gaps.is_empty());
}

#[test]
fn test_model_trainer() {
    let trainer = ModelTrainer::new();
    let samples = create_test_samples(150, "class_a");

    let model = trainer.train("test_domain", &samples).unwrap();

    assert_eq!(model.domain, "test_domain");
    assert_eq!(model.status, ModelStatus::Ready);
    assert!(model.performance.accuracy > 0.0);
}

#[test]
fn test_model_trainer_insufficient_data() {
    let trainer = ModelTrainer::new();
    let samples = create_test_samples(50, "class_a");

    let result = trainer.train("test_domain", &samples);
    assert!(result.is_err());
}

#[test]
fn test_model_switch_evaluation() {
    let manager = ModelSwitchManager::new();

    let current = DedicatedPredictionModel {
        model_id: "current".to_string(),
        domain: "test".to_string(),
        architecture: ModelArchitecture::FineTunedSmall,
        status: ModelStatus::Deployed,
        version: 1,
        training_info: TrainingInfo {
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            sample_count: 1000, // Need >= 1000 samples for confidence >= 0.7
            training_duration_seconds: 3600,
            data_hash: "hash".to_string(),
            validation_split: 0.2,
        },
        performance: ModelPerformance {
            accuracy: 0.75,
            precision: 0.74,
            recall: 0.73,
            f1_score: 0.735,
            confidence_calibration: 0.8,
            latency_ms: 100,
            evaluated_at: Utc::now(),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
        hyperparameters: HashMap::new(),
    };

    let mut candidate = current.clone();
    candidate.model_id = "candidate".to_string();
    candidate.performance.accuracy = 0.85; // 10% improvement

    let decision = manager.evaluate_switch(&current, &candidate);

    assert!(decision.should_switch);
    assert!(decision.improvement > 0.05);
}

#[test]
fn test_switch_execution() {
    let mut manager = ModelSwitchManager::new();

    manager.register_active_model("test_domain", "model_v1");

    let pending = manager
        .execute_switch("test_domain", "model_v1", "model_v2")
        .unwrap();

    assert_eq!(pending.domain, "test_domain");
    assert_eq!(pending.from_model, "model_v1");
    assert_eq!(pending.to_model, "model_v2");

    let active = manager.get_active_model("test_domain").unwrap();
    assert_eq!(active, "model_v2");
}

#[test]
fn test_rollback() {
    let mut manager = ModelSwitchManager::new();

    manager.register_active_model("test_domain", "model_v1");
    manager
        .execute_switch("test_domain", "model_v1", "model_v2")
        .unwrap();

    manager.rollback("test_domain").unwrap();

    let active = manager.get_active_model("test_domain").unwrap();
    assert_eq!(active, "model_v1");
}

#[test]
fn test_auto_rollback_check() {
    let manager = ModelSwitchManager::new();

    let baseline = ModelPerformance {
        accuracy: 0.8,
        ..Default::default()
    };

    let degraded = ModelPerformance {
        accuracy: 0.6, // 25% degradation
        ..Default::default()
    };

    assert!(manager.check_auto_rollback(&degraded, &baseline));

    let slight_degraded = ModelPerformance {
        accuracy: 0.75, // 6.25% degradation
        ..Default::default()
    };

    assert!(!manager.check_auto_rollback(&slight_degraded, &baseline));
}

#[test]
fn test_complete_system() {
    let mut system = DedicatedModelSystem::new();

    let samples = create_test_samples(150, "class_a");

    // Analyze data
    let sufficiency = system.analyze_data(&samples);
    assert!(sufficiency.is_sufficient);

    // Train model
    let model = system.train_model("test_domain", &samples).unwrap();
    system
        .switch_manager
        .register_active_model("test_domain", &model.model_id);

    // Verify
    assert_eq!(system.models.len(), 1);
    let active = system.get_active_model("test_domain").unwrap();
    assert_eq!(active.model_id, model.model_id);
}
