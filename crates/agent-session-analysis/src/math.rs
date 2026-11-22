/// Diagnostic ratios truncate toward zero and saturate to the report's integer range.
pub(crate) fn ratio_basis_points(numerator: i64, denominator: i64) -> Option<i32> {
    if denominator <= 0 {
        return None;
    }
    let ratio = i128::from(numerator) * 10_000 / i128::from(denominator);
    Some(ratio.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32)
}

/// Coverage rounds half up, unlike diagnostic ratios, and cannot exceed 100%.
pub(crate) fn coverage_percent(observed: usize, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    u8::try_from((observed.saturating_mul(100) + total / 2) / total)
        .unwrap_or(100)
        .min(100)
}

/// Select the ceiling-ranked percentile from an ascending sample slice.
/// Callers sort once so several percentiles do not copy or reorder the same samples.
pub(crate) fn percentile(ordered: &[i64], numerator: usize, denominator: usize) -> Option<i64> {
    if ordered.is_empty() || denominator == 0 {
        return None;
    }
    let index = ((ordered.len() - 1) * numerator).div_ceil(denominator);
    ordered.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_ratios_saturate_instead_of_disappearing() {
        assert_eq!(ratio_basis_points(i64::MAX, 1), Some(i32::MAX));
        assert_eq!(ratio_basis_points(i64::MIN, 1), Some(i32::MIN));
        assert_eq!(ratio_basis_points(1, 0), None);
        assert_eq!(ratio_basis_points(1, -1), None);
    }

    #[test]
    fn diagnostic_ratios_truncate_toward_zero_without_rounding() {
        assert_eq!(ratio_basis_points(1, 6), Some(1_666));
        assert_eq!(ratio_basis_points(-1, 6), Some(-1_666));
        assert_eq!(ratio_basis_points(0, 6), Some(0));
    }

    #[test]
    fn coverage_rounds_half_up_and_caps_overcounted_evidence() {
        assert_eq!(coverage_percent(0, 0), 0);
        assert_eq!(coverage_percent(0, 1), 0);
        assert_eq!(coverage_percent(1, 6), 17);
        assert_eq!(coverage_percent(1, 8), 13);
        assert_eq!(coverage_percent(2, 1), 100);
    }

    #[test]
    fn percentiles_use_ceiling_rank_without_interpolating() {
        let ordered = [10, 20, 30, 40];
        assert_eq!(percentile(&ordered, 0, 1), Some(10));
        assert_eq!(percentile(&ordered, 1, 2), Some(30));
        assert_eq!(percentile(&ordered, 9, 10), Some(40));
        assert_eq!(percentile(&ordered, 1, 1), Some(40));
        assert_eq!(percentile(&[7], 9, 10), Some(7));
        assert_eq!(percentile(&[], 1, 2), None);
        assert_eq!(percentile(&ordered, 1, 0), None);
    }
}
