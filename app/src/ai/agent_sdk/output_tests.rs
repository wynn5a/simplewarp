use comfy_table::Cell;
use serde::Serialize;
use warp_cli::agent::OutputFormat;

use super::{TableFormat, write_json_line, write_list};

#[derive(Serialize)]
struct TestItem {
    id: &'static str,
    subject: &'static str,
}

impl TableFormat for TestItem {
    fn header() -> Vec<Cell> {
        vec![Cell::new("ID"), Cell::new("SUBJECT")]
    }

    fn row(&self) -> Vec<Cell> {
        vec![Cell::new(self.id), Cell::new(self.subject)]
    }
}

#[test]
fn write_list_emits_json_for_json_output_format() {
    let mut output = Vec::new();
    let items = [TestItem {
        id: "message-1",
        subject: "Build update",
    }];

    write_list(items, OutputFormat::Json, &mut output).unwrap();

    let rendered = String::from_utf8(output).unwrap();
    assert_eq!(rendered, r#"[{"id":"message-1","subject":"Build update"}]"#);
}

#[test]
fn write_list_emits_ndjson_for_ndjson_output_format() {
    let mut output = Vec::new();
    let items = [
        TestItem {
            id: "message-1",
            subject: "Build update",
        },
        TestItem {
            id: "message-2",
            subject: "Pivot",
        },
    ];

    write_list(items, OutputFormat::Ndjson, &mut output).unwrap();

    let rendered = String::from_utf8(output).unwrap();
    assert_eq!(
        rendered,
        "{\"id\":\"message-1\",\"subject\":\"Build update\"}\n{\"id\":\"message-2\",\"subject\":\"Pivot\"}\n"
    );
}

#[test]
fn write_json_line_emits_compact_json_with_trailing_newline() {
    let mut output = Vec::new();
    let item = TestItem {
        id: "message-1",
        subject: "Build update",
    };

    write_json_line(&item, &mut output).unwrap();

    let rendered = String::from_utf8(output).unwrap();
    assert_eq!(
        rendered,
        "{\"id\":\"message-1\",\"subject\":\"Build update\"}\n"
    );
}
