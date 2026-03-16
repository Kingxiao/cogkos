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
