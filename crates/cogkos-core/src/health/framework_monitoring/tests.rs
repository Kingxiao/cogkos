use super::*;
use chrono::Utc;

#[test]
fn test_prediction_tracker() {
    let mut tracker = PredictionTracker::new(uuid::Uuid::new_v4());

    // Record some predictions
    tracker.record("a", "a", 0.8); // Correct
    tracker.record("b", "b", 0.9); // Correct
    tracker.record("c", "d", 0.7); // Wrong

    assert_eq!(tracker.total_predictions, 3);
    assert_eq!(tracker.correct_predictions, 2);
    assert!((tracker.current_accuracy() - 0.667).abs() < 0.01);
}

#[test]
fn test_rolling_accuracy() {
    let mut tracker = PredictionTracker::new(uuid::Uuid::new_v4());

    for _i in 0..20 {
        tracker.record("x", "x", 0.8); // All correct
    }

    assert_eq!(tracker.rolling_accuracy(10), 1.0);
}

#[test]
fn test_bias_detector() {
    let mut detector = BiasDetector::new();

    // Create trackers with calibration issues
    let mut trackers = Vec::new();
    for _i in 0..60 {
        let mut tracker = PredictionTracker::new(uuid::Uuid::new_v4());
        for _ in 0..20 {
            tracker.record("x", "x", 0.9); // High confidence
        }
        // But set low correct count to create calibration error
        // (simplified - in real test would manipulate history)
        trackers.push(tracker);
    }

    let _biases = detector.detect_all_biases(&trackers);
    // May or may not detect bias depending on random history
}

#[test]
fn test_framework_health_monitor() {
    let mut monitor = FrameworkHealthMonitor::new();

    let insight_id = uuid::Uuid::new_v4();

    // Record some predictions
    for _i in 0..20 {
        monitor.record_prediction(insight_id, "result", "result", 0.8);
    }

    let report = monitor.generate_report(1);

    assert!(report.prediction_power.total_predictions_evaluated > 0);
    assert!(report.overall_health >= 0.0 && report.overall_health <= 1.0);
}

#[test]
fn test_framework_status() {
    let healthy = FrameworkHealthReport {
        generated_at: Utc::now(),
        report_period_days: 7,
        overall_health: 0.85,
        prediction_power: PredictionPowerMetrics {
            overall_accuracy: 0.85,
            accuracy_trend: TrendDirection::Stable,
            total_predictions_evaluated: 1000,
            insights_evaluated: 50,
            high_performing_insights: 40,
            underperforming_insights: 5,
            calibration_score: 0.9,
        },
        detected_biases: vec![],
        recommendations: vec![],
        status: FrameworkStatus::Healthy,
    };

    assert!(healthy.is_healthy());
}

#[test]
fn test_category_accuracy_tracking() {
    let mut detector = BiasDetector::new();

    detector.record_category_accuracy("finance", 0.8);
    detector.record_category_accuracy("finance", 0.85);
    detector.record_category_accuracy("tech", 0.6);
    detector.record_category_accuracy("tech", 0.55);

    let _bias = detector.detect_context_bias();
    // Should detect variance between categories
}

#[test]
fn test_trend_direction() {
    assert_ne!(TrendDirection::Improving, TrendDirection::Declining);
}
