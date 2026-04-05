//! Decay model unit tests

use cogkos_core::evolution::decay::{
    calculate_decay, calculate_decay_with_revalidation, calculate_effective_durability,
    needs_revalidation,
};

#[test]
fn test_calculate_decay_basic() {
    let result = calculate_decay(0.8, 0.01, 24.0, 0.5);
    // After 24h with lambda=0.01 and activation=0.5
    assert!(result > 0.4 && result < 0.8);
}

#[test]
fn test_calculate_decay_zero_confidence() {
    let result = calculate_decay(0.0, 0.01, 24.0, 0.5);
    assert_eq!(result, 0.0);
}

#[test]
fn test_calculate_decay_high_activation_protects() {
    let low_act = calculate_decay(0.8, 0.01, 100.0, 0.1);
    let high_act = calculate_decay(0.8, 0.01, 100.0, 1.0);
    assert!(high_act > low_act, "Higher activation should slow decay");
}

#[test]
fn test_calculate_decay_with_revalidation_boost() {
    let decayed = calculate_decay(0.8, 0.01, 100.0, 0.5);
    let boosted = calculate_decay_with_revalidation(0.8, 0.01, 100.0, 0.5, 0.5);
    assert!(
        boosted > decayed,
        "Revalidation boost should increase confidence"
    );
}

#[test]
fn test_needs_revalidation_low_confidence() {
    assert!(needs_revalidation(0.2, 0.3, 10.0, 720.0));
}

#[test]
fn test_needs_revalidation_old_age() {
    assert!(needs_revalidation(0.8, 0.3, 1000.0, 720.0));
}

#[test]
fn test_needs_revalidation_fresh_high_confidence() {
    assert!(!needs_revalidation(0.8, 0.3, 10.0, 720.0));
}

#[test]
fn test_calculate_effective_durability() {
    let base = calculate_effective_durability(0.5, 0, 0);
    let with_access = calculate_effective_durability(0.5, 100, 0);
    let with_both = calculate_effective_durability(0.5, 100, 50);
    assert!(with_access > base);
    assert!(with_both > with_access);
    assert!(with_both <= 1.0);
}

// ── P1: S4 Decay Curve Precision ─────────────────────────────

#[test]
fn test_decay_exponential_formula_exact() {
    // Formula: confidence × exp(-lambda/activation × time)
    // confidence=1.0, lambda=0.01, time=100h, activation=0.5
    // expected = 1.0 × exp(-0.01/0.5 × 100) = exp(-2.0) ≈ 0.1353
    let result = calculate_decay(1.0, 0.01, 100.0, 0.5);
    let expected = (-2.0_f64).exp();
    assert!(
        (result - expected).abs() < 0.001,
        "Decay should follow exp(-lambda/activation * t): got {}, expected {}",
        result,
        expected
    );
}

#[test]
fn test_decay_half_life_calculation() {
    // Half-life = ln(2) * activation / lambda
    // With lambda=0.01, activation=0.5: half_life = 0.693/0.02 = 34.66h
    let half_life = 0.5_f64.ln().abs() / (0.01 / 0.5);
    let result = calculate_decay(1.0, 0.01, half_life, 0.5);
    assert!(
        (result - 0.5).abs() < 0.01,
        "At half-life ({:.1}h), confidence should be ~0.5, got {}",
        half_life,
        result
    );
}

#[test]
fn test_decay_one_week_default_lambda() {
    // Default lambda=0.001, activation=0.5, 168h (1 week)
    // exp(-0.001/0.5 * 168) = exp(-0.336) ≈ 0.7146
    let result = calculate_decay(0.9, 0.001, 168.0, 0.5);
    let expected = 0.9 * (-0.336_f64).exp();
    assert!(
        (result - expected).abs() < 0.01,
        "1 week decay should be ~{:.3}, got {:.3}",
        expected,
        result
    );
}

#[test]
fn test_decay_activation_protection_quantitative() {
    // Same time, different activation: act=0.1 vs act=1.0
    // act=0.1: exp(-0.01/0.1 * 100) = exp(-10) ≈ 0.0000454
    // act=1.0: exp(-0.01/1.0 * 100) = exp(-1) ≈ 0.3679
    let low = calculate_decay(1.0, 0.01, 100.0, 0.1);
    let high = calculate_decay(1.0, 0.01, 100.0, 1.0);
    assert!(
        low < 0.001,
        "Low activation should decay almost to zero: {}",
        low
    );
    assert!(high > 0.35, "High activation should retain >35%: {}", high);
    assert!(
        high / low > 1000.0,
        "10x activation should give >1000x protection ratio"
    );
}

// ── P1: D3 Knowledge Lifecycle — Revalidation Trigger ────────

#[test]
fn test_revalidation_boundary_conditions() {
    // Exactly at threshold: confidence=0.3 with threshold=0.3, young claim
    assert!(
        !needs_revalidation(0.3, 0.3, 5.0, 720.0),
        "At threshold, young claim should not trigger"
    );
    // Just below threshold
    assert!(
        needs_revalidation(0.29, 0.3, 5.0, 720.0),
        "Below threshold should trigger"
    );
    // Old claim above threshold
    assert!(
        needs_revalidation(0.8, 0.3, 721.0, 720.0),
        "Old claim should trigger regardless"
    );
}

// ── P1: D7 Health Score Formula ──────────────────────────────

#[test]
fn test_health_score_formula_weights() {
    // health_score = PA*0.4 + CRR*0.3 + (KD/7)*0.15 + (IGR/10)*0.15
    // Perfect scores: PA=1.0, CRR=1.0, KD=7, IGR=10 → 1.0
    let perfect =
        1.0 * 0.4 + 1.0 * 0.3 + (7.0 / 7.0_f64).min(1.0) * 0.15 + (10.0 / 10.0_f64).min(1.0) * 0.15;
    assert!(
        (perfect - 1.0).abs() < 0.001,
        "Perfect scores should give 1.0"
    );

    // Zero everything: 0.0
    let zero: f64 = 0.0 * 0.4 + 0.0 * 0.3 + 0.0 * 0.15 + 0.0 * 0.15;
    assert!((zero - 0.0).abs() < 0.001, "Zero scores should give 0.0");

    // Typical: PA=1.0, CRR=0.0, KD=2, IGR=0 → 0.4 + 0 + 0.042 + 0 = 0.443
    let typical = 1.0 * 0.4 + 0.0 * 0.3 + (2.0 / 7.0_f64).min(1.0) * 0.15 + 0.0 * 0.15;
    assert!(
        (typical - 0.443).abs() < 0.01,
        "Typical case should be ~0.443, got {}",
        typical
    );
}

#[test]
fn test_health_score_warning_threshold() {
    // health_score < 0.5 should trigger warning
    // PA=1.0, CRR=0.0, KD=1, IGR=0 → 0.4 + 0 + 0.021 + 0 = 0.421
    let score = 1.0 * 0.4 + 0.0 * 0.3 + (1.0 / 7.0_f64).min(1.0) * 0.15 + 0.0 * 0.15;
    assert!(
        score < 0.5,
        "This configuration should be below warning threshold"
    );

    // PA=0.8, CRR=0.5, KD=3, IGR=2 → 0.32 + 0.15 + 0.064 + 0.03 = 0.564
    let healthy =
        0.8 * 0.4 + 0.5 * 0.3 + (3.0 / 7.0_f64).min(1.0) * 0.15 + (2.0 / 10.0_f64).min(1.0) * 0.15;
    assert!(
        healthy >= 0.5,
        "This configuration should be above warning threshold: {}",
        healthy
    );
}
