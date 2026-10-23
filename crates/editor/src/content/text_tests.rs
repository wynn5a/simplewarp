use markdown_parser::{CodeBlockText, FormattedTable};

use super::{BufferBlockItem, CodeBlockType, MarkdownStyle, format_image_markdown};

#[test]
fn test_formatted_table_round_trip() {
    let input = "Name\tAge\nAlice\t30\nBob\t25\n";
    let table = FormattedTable::from_internal_format(input);
    assert_eq!(table.headers.len(), 2);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.to_internal_format(), input);
}

#[test]
fn test_formatted_table_single_column() {
    let input = "Header\nValue";
    let table = FormattedTable::from_internal_format(input);
    assert_eq!(table.headers.len(), 1);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.to_internal_format(), "Header\nValue\n");
}

#[test]
fn test_formatted_table_empty_input() {
    let table = FormattedTable::from_internal_format("");
    assert!(table.headers.is_empty());
    assert!(table.rows.is_empty());
}

#[test]
fn test_mermaid_code_block_type() {
    let markdown = CodeBlockText {
        lang: "mermaid".to_string(),
        code: "graph TD\nA --> B\n".to_string(),
    };

    assert_eq!(CodeBlockType::from(&markdown), CodeBlockType::Mermaid);
}

#[test]
fn test_formatted_table_normalize_shape() {
    let input = "A\tB\tC\nX";
    let mut table = FormattedTable::from_internal_format(input);
    assert_eq!(table.rows[0].len(), 1);
    table.normalize_shape();
    assert_eq!(table.headers.len(), 3);
    assert_eq!(table.rows[0].len(), 3);
}

#[test]
fn format_image_markdown_preserves_title() {
    // No title -> canonical pre-title form.
    assert_eq!(
        format_image_markdown("alt", "src.png", None),
        "![alt](src.png)"
    );

    // Empty title is equivalent to no title (product invariant 4).
    assert_eq!(
        format_image_markdown("alt", "src.png", Some("")),
        "![alt](src.png)"
    );

    // Non-empty title is re-serialized with double quotes.
    assert_eq!(
        format_image_markdown("alt", "src.png", Some("caption")),
        "![alt](src.png \"caption\")"
    );

    // Literal double quotes in the title are escaped with a backslash so the
    // round-trip remains lossless.
    assert_eq!(
        format_image_markdown("alt", "src.png", Some("a \"quoted\" caption")),
        "![alt](src.png \"a \\\"quoted\\\" caption\")"
    );
}

#[test]
fn buffer_block_image_as_markdown_preserves_title() {
    let untitled = BufferBlockItem::Image {
        alt_text: "A dog".to_string(),
        source: "dog.png".to_string(),
        title: None,
    };
    assert_eq!(
        &*untitled.as_markdown(MarkdownStyle::Internal),
        "![A dog](dog.png)"
    );

    let titled = BufferBlockItem::Image {
        alt_text: "A dog".to_string(),
        source: "dog.png".to_string(),
        title: Some("Rex, my dog".to_string()),
    };
    assert_eq!(
        &*titled.as_markdown(MarkdownStyle::Internal),
        "![A dog](dog.png \"Rex, my dog\")"
    );
}

#[test]
fn buffer_block_image_partial_eq_considers_title() {
    let untitled = BufferBlockItem::Image {
        alt_text: "A dog".to_string(),
        source: "dog.png".to_string(),
        title: None,
    };
    let titled = BufferBlockItem::Image {
        alt_text: "A dog".to_string(),
        source: "dog.png".to_string(),
        title: Some("Rex".to_string()),
    };
    assert_ne!(untitled, titled);

    let titled_again = BufferBlockItem::Image {
        alt_text: "A dog".to_string(),
        source: "dog.png".to_string(),
        title: Some("Rex".to_string()),
    };
    assert_eq!(titled, titled_again);
}
