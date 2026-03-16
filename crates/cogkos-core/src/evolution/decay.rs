/// Calculate decayed confidence over time
/// Uses exponential decay modulated by activation weight
///
/// Formula: new_confidence = confidence * exp(-lambda * time_delta / activation_weight)
///
/// Higher activation_weight slows decay (knowledge that is accessed frequently persists longer)
pub fn calculate_decay(
    confidence: f64,
    lambda: f64, // Base decay rate (e.g., 0.01 for slow decay)
    time_delta_hours: f64,
    activation_weight: f64,
) -> f64 {
    if confidence <= 0.0 {
        return 0.0;
    }

    // Activation weight protects against decay (0.5 = normal, 1.0 = max protection)
    let protection_factor = activation_weight.max(0.1); // Min 0.1 to prevent division by zero
    let effective_lambda = lambda / protection_factor;

    let decay_factor = (-effective_lambda * time_delta_hours).exp();
    let new_confidence = confidence * decay_factor;

    new_confidence.clamp(0.0, 1.0)
}

/// Calculate decay with revalidation boost
/// If the knowledge is revalidated (e.g., by new confirming evidence),
/// the confidence gets a boost
pub fn calculate_decay_with_revalidation(
    confidence: f64,
    lambda: f64,
    time_delta_hours: f64,
    activation_weight: f64,
    revalidation_boost: f64, // 0.0 to 1.0 boost from confirmation
) -> f64 {
    let decayed = calculate_decay(confidence, lambda, time_delta_hours, activation_weight);

    // Apply revalidation boost
    let boosted = decayed + revalidation_boost * (1.0 - decayed);
    boosted.clamp(0.0, 1.0)
}

/// Check if knowledge needs revalidation
pub fn needs_revalidation(
    current_confidence: f64,
    threshold: f64,
    last_validated_hours: f64,
    max_age_hours: f64,
) -> bool {
    current_confidence < threshold || last_validated_hours > max_age_hours
}

/// Calculate effective durability based on usage patterns
pub fn calculate_effective_durability(
    base_durability: f64,
    access_count: u64,
    confirmation_count: u64,
) -> f64 {
    let usage_bonus = (access_count as f64 * 0.01).min(0.2); // Max 0.2 bonus from usage
    let confirmation_bonus = (confirmation_count as f64 * 0.02).min(0.3); // Max 0.3 from confirmation

    (base_durability + usage_bonus + confirmation_bonus).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_decay_basic() {
        let confidence = 0.8;
        let lambda = 0.01; // 1% decay per hour
        let time_delta = 24.0; // 1 day
        let activation = 0.5; // Normal activation

        let decayed = calculate_decay(confidence, lambda, time_delta, activation);

        // Should decay but not too much
        assert!(decayed < confidence);
        assert!(decayed > 0.4);
    }

    #[test]
    fn test_calculate_decay_with_high_activation() {
        let confidence = 0.8;
        let lambda = 0.01;
        let time_delta = 24.0;

        let decayed_normal = calculate_decay(confidence, lambda, time_delta, 0.5);
        let decayed_protected = calculate_decay(confidence, lambda, time_delta, 1.0);

        // High activation should decay slower
        assert!(decayed_protected > decayed_normal);
    }

    #[test]
    fn test_calculate_decay_with_revalidation() {
        let confidence = 0.5;
        let lambda = 0.01;
        let time_delta = 100.0; // Long time
        let activation = 0.5;

        let decayed = calculate_decay(confidence, lambda, time_delta, activation);
        let boosted =
            calculate_decay_with_revalidation(confidence, lambda, time_delta, activation, 0.3);

        assert!(boosted > decayed);
    }

    #[test]
    fn test_needs_revalidation() {
        assert!(needs_revalidation(0.3, 0.5, 10.0, 100.0)); // Below threshold
        assert!(needs_revalidation(0.8, 0.5, 200.0, 100.0)); // Too old
        assert!(!needs_revalidation(0.8, 0.5, 50.0, 100.0)); // OK
    }
}
