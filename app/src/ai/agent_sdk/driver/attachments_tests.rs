use std::io::Write;

use tempfile::NamedTempFile;

use super::*;

#[test]
fn process_attachment_text_file() {
    let mut f = NamedTempFile::with_suffix(".txt").unwrap();
    write!(f, "hello world").unwrap();

    let result = process_attachment(&f.path().to_path_buf(), 0).unwrap();
    assert_eq!(
        result.file_name,
        f.path().file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(result.mime_type, "text/plain");
    assert_eq!(
        general_purpose::STANDARD.decode(&result.data).unwrap(),
        b"hello world"
    );
}

#[test]
fn process_attachment_too_large() {
    let mut f = NamedTempFile::with_suffix(".bin").unwrap();
    let data = vec![0u8; MAX_ATTACHMENT_SIZE_BYTES + 1];
    f.write_all(&data).unwrap();

    let err = process_attachment(&f.path().to_path_buf(), 0).unwrap_err();
    assert!(err.to_string().contains("too large"));
}

#[test]
fn process_attachment_nonexistent_file() {
    let path = std::path::PathBuf::from("/tmp/nonexistent_attachment_test_file.xyz");
    let err = process_attachment(&path, 0).unwrap_err();
    assert!(err.to_string().contains("Failed to read"));
}
