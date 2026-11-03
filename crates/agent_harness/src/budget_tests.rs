use super::*;

#[test]
fn estimate_is_a_quarter_of_the_characters_rounded_up() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("abc"), 1);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abcde"), 2);
}

#[test]
fn estimate_counts_characters_not_bytes() {
    // Four three-byte characters are one token, not three.
    assert_eq!(estimate_tokens("日本語文"), 1);
}

#[test]
fn a_report_replaces_the_previous_usage_and_its_estimates() {
    let mut usage = ContextUsage::default();
    usage.record_report(10_000, 500);
    usage.set_estimate(2_000);
    assert_eq!(usage.used_tokens(), 12_500);

    usage.record_report(13_000, 400);
    assert_eq!(usage.used_tokens(), 13_400);
}

#[test]
fn a_new_estimate_replaces_the_last_one() {
    let mut usage = ContextUsage::default();
    usage.record_report(1_000, 0);
    usage.set_estimate(100);
    // The same request sent again.
    usage.set_estimate(100);
    assert_eq!(usage.used_tokens(), 1_100);
}

#[test]
fn estimating_from_characters_matches_estimating_from_text() {
    assert_eq!(estimate_tokens_for_chars(0), 0);
    assert_eq!(estimate_tokens_for_chars(5), estimate_tokens("abcde"));
}

#[test]
fn no_report_means_no_usage_to_show() {
    let mut usage = ContextUsage::default();
    assert!(!usage.has_report());
    usage.set_estimate(100);
    assert!(!usage.has_report());
    usage.record_report(100, 10);
    assert!(usage.has_report());
}

#[test]
fn a_restored_total_counts_as_a_report() {
    let usage = ContextUsage::from_tokens(42_000);
    assert!(usage.has_report());
    assert_eq!(usage.used_tokens(), 42_000);
}

#[test]
fn fraction_used_divides_by_the_window() {
    let usage = ContextUsage::from_tokens(50_000);
    assert_eq!(usage.fraction_used(200_000), Some(0.25));
    assert_eq!(usage.fraction_used(0), None);
    assert_eq!(ContextUsage::from_tokens(300).fraction_used(200), Some(1.5));
}

#[test]
fn the_warning_starts_at_eighty_percent() {
    assert!(!ContextUsage::from_tokens(159_999).is_near_limit(200_000));
    assert!(ContextUsage::from_tokens(160_000).is_near_limit(200_000));
    assert!(!ContextUsage::from_tokens(1).is_near_limit(0));
}

#[test]
fn the_limit_caps_the_model_window() {
    assert_eq!(effective_window(Some(200_000), Some(64_000)), Some(64_000));
    assert_eq!(effective_window(Some(32_000), Some(64_000)), Some(32_000));
}

#[test]
fn either_window_alone_is_used() {
    assert_eq!(effective_window(Some(200_000), None), Some(200_000));
    assert_eq!(effective_window(None, Some(64_000)), Some(64_000));
    assert_eq!(effective_window(None, None), None);
}

#[test]
fn a_zero_limit_is_no_limit() {
    assert_eq!(effective_window(Some(200_000), Some(0)), Some(200_000));
    assert_eq!(effective_window(None, Some(0)), None);
}

#[test]
fn tokens_are_formatted_compactly() {
    assert_eq!(format_tokens(0), "0");
    assert_eq!(format_tokens(950), "950");
    assert_eq!(format_tokens(42_300), "42k");
    assert_eq!(format_tokens(42_500), "43k");
    assert_eq!(format_tokens(200_000), "200k");
    assert_eq!(format_tokens(999_400), "999k");
    assert_eq!(format_tokens(999_600), "1M");
    assert_eq!(format_tokens(1_000_000), "1M");
    assert_eq!(format_tokens(1_048_576), "1M");
    assert_eq!(format_tokens(1_250_000), "1.3M");
}
