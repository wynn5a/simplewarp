use super::*;

#[test]
fn sanitized_basename_accepts_plain_filename() {
    assert_eq!(
        sanitized_basename("report.txt"),
        Some("report.txt".to_string())
    );
}

#[test]
fn sanitized_basename_extracts_from_path() {
    assert_eq!(
        sanitized_basename("outputs/report.txt"),
        Some("report.txt".to_string())
    );
}
