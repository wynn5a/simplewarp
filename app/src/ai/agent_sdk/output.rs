use std::io::Write as _;

use anyhow::Context;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, ContentArrangement, Table};
use serde::Serialize;
use tabwriter::TabWriter;
use warp_cli::agent::OutputFormat;

pub fn standard_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// Trait for types that can be printed as a table.
pub trait TableFormat {
    fn header() -> Vec<Cell>;

    fn row(&self) -> Vec<Cell>;
}

/// Print a list of items to stdout, respecting the `output_format`.
pub fn print_list<I, T>(items: I, output_format: OutputFormat)
where
    I: IntoIterator<Item = T>,
    T: TableFormat + Serialize,
{
    if let Err(err) = write_list(items, output_format, &mut std::io::stdout()) {
        // If we can't write to stdout, try reporting to the log file.
        log::warn!("Unable to write to stdout: {err}");
    }
}

/// Write a serializable value to `output` as a single-line JSON record.
pub fn write_json_line<T, W>(value: &T, mut output: W) -> anyhow::Result<()>
where
    T: Serialize,
    W: std::io::Write,
{
    serde_json::to_writer(&mut output, value).context("unable to write JSON output")?;
    writeln!(&mut output)?;
    Ok(())
}
/// Write a list of items to `output`, respecting the `output_format`.
pub fn write_list<I, T, W>(
    items: I,
    output_format: OutputFormat,
    mut output: W,
) -> anyhow::Result<()>
where
    I: IntoIterator<Item = T>,
    T: TableFormat + Serialize,
    W: std::io::Write,
{
    match output_format {
        OutputFormat::Json => {
            let items = items.into_iter().collect::<Vec<_>>();
            serde_json::to_writer(&mut output, &items).context("unable to write JSON output")
        }
        OutputFormat::Ndjson => {
            for item in items {
                write_json_line(&item, &mut output)?;
            }
            Ok(())
        }
        OutputFormat::Pretty => {
            // Use comfy-table to print a table with terminal formatting.
            let mut table = standard_table();
            table.set_header(T::header());
            for item in items {
                table.add_row(T::row(&item));
            }
            writeln!(&mut output, "{table}")?;
            Ok(())
        }
        OutputFormat::Text => {
            // Print a plain-text table.
            let mut tw = TabWriter::new(output);

            for (idx, column) in T::header().iter().enumerate() {
                if idx > 0 {
                    write!(&mut tw, "\t")?;
                }
                write!(&mut tw, "{}", column.content())?;
            }
            writeln!(&mut tw)?;

            for item in items {
                for (idx, column) in T::row(&item).iter().enumerate() {
                    if idx > 0 {
                        write!(&mut tw, "\t")?;
                    }
                    write!(&mut tw, "{}", column.content())?;
                }
                writeln!(&mut tw)?;
            }
            tw.flush()?;
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
