use std::iter::Peekable;

/// Split a table row into cells, handling escaped pipes (`\|`) as literal pipe characters.
fn split_cells_escaped(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    let mut cells = Vec::new();
    let mut current_cell = String::new();
    let mut chars = trimmed.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            current_cell.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(current_cell.trim().to_string());
            current_cell = String::new();
        } else {
            current_cell.push(c);
        }
    }
    cells.push(current_cell.trim().to_string());
    cells
}

/// Returns true if the line looks like a GFM pipe-table separator row,
/// e.g. `| --- | ---: | :---: |`.
fn is_gfm_table_separator_row(row: &str) -> bool {
    let trimmed = row.trim();
    if trimmed.is_empty() || !trimmed.contains('|') {
        return false;
    }

    let mut contains_separator_cell = false;
    // Split row into individual cells.
    for cell in trimmed.split('|').map(|c| c.trim()) {
        if cell.is_empty() {
            continue;
        }

        // `:` are used to indicate a column's horizontal alignment (e.g. `:--:` for center).
        let dashes = cell.trim_matches(':').trim();
        if dashes.is_empty() {
            return false;
        }
        if !dashes.chars().all(|c| c == '-') {
            return false;
        }
        contains_separator_cell = true;
    }
    contains_separator_cell
}

/// Attempts to parse a GFM table starting from `header_line`.
///
/// If the next line in `lines` is a valid GFM separator row, this consumes all
/// subsequent table rows and returns the raw table lines.
/// The `should_stop` predicate is called on each candidate row to allow the caller
/// to halt parsing early (e.g., when encountering a fenced code block).
///
/// Returns `None` if `header_line` and the next line don't form a valid table start.
pub fn maybe_collect_gfm_table_lines<'a, I>(
    header_line: &str,
    lines: &mut Peekable<I>,
    should_stop: impl Fn(&str) -> bool,
) -> Option<Vec<String>>
where
    I: Iterator<Item = &'a str>,
{
    let header_trimmed = header_line.trim();
    let has_leading_or_trailing_pipe =
        header_trimmed.starts_with('|') || header_trimmed.ends_with('|');
    let has_at_least_two_pipes = header_trimmed.matches('|').count() >= 2;
    if !has_leading_or_trailing_pipe || !has_at_least_two_pipes {
        return None;
    }

    let separator = lines
        .next_if(|line| is_gfm_table_separator_row(line))?
        .to_owned();

    let header_column_count = split_cells_escaped(header_trimmed).len();
    let separator_column_count = split_cells_escaped(&separator).len();
    if header_column_count != separator_column_count {
        return None;
    }

    let mut table_lines = vec![header_line.to_owned(), separator];

    while let Some(next_line) = lines.peek() {
        let is_blank = next_line.trim().is_empty();
        let is_end_of_section = should_stop(next_line);
        let row_column_count = split_cells_escaped(next_line).len();
        let has_wrong_column_count = row_column_count != header_column_count;
        if is_blank || is_end_of_section || has_wrong_column_count {
            break;
        }
        table_lines.push(lines.next().expect("peeked line must exist").to_owned());
    }

    Some(table_lines)
}
