/// A generic columnar table renderer that prints an indented, padded table to stdout.
///
/// Columns `0..padded_cols` are right-padded to their maximum width. Any columns
/// at index `padded_cols` or beyond are printed unpadded, joined by two spaces.
///
/// # Design notes
///
/// - `headers` may have length `padded_cols` (no trailing unpadded header shown) or
///   length `padded_cols + 1` (last header printed unpadded in the header row).
/// - Data rows may have length `padded_cols` (no trailing) or more (trailing unpadded).
/// - When all columns need to be padded (e.g. `print_outdated`), set `padded_cols`
///   equal to the total column count and data rows should not carry extra columns.
///
/// # Example — trailing column without a header
///
/// ```
/// use cmx::table::Table;
///
/// let table = Table {
///     headers: vec!["Name", "Installed"],
///     padded_cols: 2,
///     rows: vec![
///         vec!["my-agent".to_string(), "1.0.0".to_string(), "ok".to_string()],
///     ],
/// };
/// table.print();
/// // Output:
/// //   Name      Installed
/// //   --------  ---------
/// //   my-agent  1.0.0      ok
/// ```
///
/// # Example — trailing column with a header
///
/// ```
/// use cmx::table::Table;
///
/// let table = Table {
///     headers: vec!["Name", "Description"],
///     padded_cols: 1,
///     rows: vec![
///         vec!["my-agent".to_string(), "Does cool things.".to_string()],
///     ],
/// };
/// table.print();
/// ```
pub struct Table {
    /// Column header labels. `headers[0..padded_cols]` are padded; any header
    /// beyond `padded_cols` is printed unpadded in the header and separator rows.
    pub headers: Vec<&'static str>,
    /// Number of columns that receive right-padding. Must be `<= headers.len()`.
    pub padded_cols: usize,
    /// Row data. Each row may have `padded_cols` cells (no trailing) or more
    /// (extra cells printed unpadded after the padded block).
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// Computes the display width for each padded column.
    ///
    /// Each width is `max(header.len(), max data cell len across all rows)` for
    /// columns `0..padded_cols`. The returned `Vec` has length `padded_cols`.
    pub fn column_widths(&self) -> Vec<usize> {
        (0..self.padded_cols)
            .map(|col| {
                let header_len = self.headers.get(col).map_or(0, |h| h.len());
                let data_max = self
                    .rows
                    .iter()
                    .filter_map(|row| row.get(col))
                    .map(String::len)
                    .max()
                    .unwrap_or(0);
                header_len.max(data_max)
            })
            .collect()
    }

    /// Renders the table to a `String` with a 2-space leading indent.
    ///
    /// Output format:
    /// - Header row: padded columns followed by any extra header (unpadded)
    /// - Separator row: dashes per padded column width + any extra header dashes (unpadded)
    /// - Data rows: padded columns followed by extra data cells (unpadded)
    pub fn render(&self) -> String {
        use std::fmt::Write as FmtWrite;

        let widths = self.column_widths();
        let mut out = String::new();

        // Header row
        let mut header_parts: Vec<String> = self.headers[..self.padded_cols]
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let w = widths[i];
                format!("{h:<w$}")
            })
            .collect();
        // Any header beyond padded_cols is printed unpadded.
        if let Some(trailing_header) = self.headers.get(self.padded_cols) {
            header_parts.push(trailing_header.to_string());
        }
        let _ = writeln!(out, "  {}", header_parts.join("  "));

        // Separator row
        let mut sep_parts: Vec<String> = widths
            .iter()
            .map(|&w| {
                let dashes = "-".repeat(w);
                format!("{dashes:<w$}")
            })
            .collect();
        if let Some(trailing_header) = self.headers.get(self.padded_cols) {
            sep_parts.push("-".repeat(trailing_header.len()));
        }
        let _ = writeln!(out, "  {}", sep_parts.join("  "));

        // Data rows
        for row in &self.rows {
            let mut parts: Vec<String> = row[..self.padded_cols.min(row.len())]
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let w = widths.get(i).copied().unwrap_or(0);
                    format!("{cell:<w$}")
                })
                .collect();
            // Extra columns beyond padded_cols are unpadded trailing fields.
            for cell in row.iter().skip(self.padded_cols) {
                parts.push(cell.clone());
            }
            let _ = writeln!(out, "  {}", parts.join("  "));
        }

        out
    }

    /// Prints the table to stdout. Delegates to [`render`](Self::render).
    pub fn print(&self) {
        print!("{}", self.render());
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table(
        headers: Vec<&'static str>,
        padded_cols: usize,
        rows: Vec<Vec<&'static str>>,
    ) -> Table {
        Table {
            headers,
            padded_cols,
            rows: rows.into_iter().map(|r| r.into_iter().map(str::to_string).collect()).collect(),
        }
    }

    // --- column_widths ---

    #[test]
    fn column_widths_uses_header_length_as_minimum() {
        let t = make_table(vec!["Name", "Version"], 2, vec![vec!["a", "b"]]);
        let widths = t.column_widths();
        // "Name" = 4, "Version" = 7; data "a"=1, "b"=1 — headers win
        assert_eq!(widths, vec![4, 7]);
    }

    #[test]
    fn column_widths_uses_data_when_wider_than_header() {
        let t = make_table(vec!["N", "V"], 2, vec![vec!["long-name", "2.0.0"]]);
        let widths = t.column_widths();
        assert_eq!(widths[0], 9); // "long-name" = 9 > "N" = 1
        assert_eq!(widths[1], 5); // "2.0.0" = 5 > "V" = 1
    }

    #[test]
    fn column_widths_uses_max_across_all_rows() {
        let t =
            make_table(vec!["Col"], 1, vec![vec!["short"], vec!["a-very-long-value"], vec!["mid"]]);
        let widths = t.column_widths();
        assert_eq!(widths[0], 17); // "a-very-long-value" = 17
    }

    #[test]
    fn column_widths_empty_rows_falls_back_to_header_length() {
        let t = make_table(vec!["Name", "Version"], 2, vec![]);
        let widths = t.column_widths();
        assert_eq!(widths, vec![4, 7]);
    }

    #[test]
    fn column_widths_ignores_trailing_columns_beyond_padded_cols() {
        // padded_cols=1 means only "Name" is padded; "Description" is not measured
        let t = make_table(
            vec!["Name", "Description"],
            1,
            vec![vec![
                "a",
                "some very long description that should not affect widths",
            ]],
        );
        let widths = t.column_widths();
        // Only col 0 is padded; "Description" does not affect widths
        assert_eq!(widths.len(), 1);
        assert_eq!(widths[0], 4); // max("Name"=4, "a"=1) = 4
    }

    // --- trailing column without a header ---

    #[test]
    fn trailing_data_column_without_header_is_unpadded() {
        // 4 padded cols, data rows have a 5th (status) with no header
        let t = make_table(
            vec!["Name", "Installed", "Source", "Available"],
            4,
            vec![vec!["my-agent", "1.0.0", "guidelines", "2.0.0", "update"]],
        );
        let widths = t.column_widths();
        // padded cols: max(4,8), max(9,5), max(6,10), max(9,5)
        assert_eq!(widths, vec![8, 9, 10, 9]);
        // widths does not include col 4 (status)
        assert_eq!(widths.len(), 4);
    }

    // --- trailing column with a header ---

    #[test]
    fn trailing_header_printed_without_padding() {
        // padded_cols=1, headers has 2 entries: "Name" (padded) + "Description" (unpadded)
        let t = make_table(vec!["Name", "Description"], 1, vec![vec!["a", "text"]]);
        // widths[0] = max("Name"=4, "a"=1) = 4; "Description" is unpadded
        let widths = t.column_widths();
        assert_eq!(widths, vec![4]);
    }

    // --- all columns padded ---

    #[test]
    fn all_columns_padded_when_padded_cols_equals_col_count() {
        let t = make_table(vec!["Name", "Status"], 2, vec![vec!["my-agent", "ok"]]);
        let widths = t.column_widths();
        // Both cols padded; padded_cols == headers.len()
        assert_eq!(widths, vec![8, 6]); // max("Name"=4,"my-agent"=8), max("Status"=6,"ok"=2)
    }

    // --- multi-row alignment ---

    #[test]
    fn multi_row_padded_columns_align() {
        let t = make_table(
            vec!["Name", "Version"],
            2,
            vec![vec!["short", "1.0"], vec!["a-longer-name", "10.0.0"]],
        );
        let widths = t.column_widths();
        // widths[0] = max(4, 5, 13) = 13; widths[1] = max(7, 3, 6) = 7
        assert_eq!(widths, vec![13, 7]);
    }

    // --- render ---

    #[test]
    fn render_header_row_with_separator() {
        let t = make_table(vec!["Name", "Version"], 2, vec![]);
        let out = t.render();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        // "Name" padded to 4, "Version" padded to 7, joined by 2 spaces
        assert_eq!(lines[0], "  Name  Version");
        assert_eq!(lines[1], "  ----  -------");
    }

    #[test]
    fn render_data_rows_are_padded() {
        let t = make_table(
            vec!["Name", "Version"],
            2,
            vec![vec!["short", "1.0"], vec!["a-longer-name", "10.0.0"]],
        );
        let out = t.render();
        let lines: Vec<&str> = out.lines().collect();
        // header + separator + 2 data rows = 4
        assert_eq!(lines.len(), 4);
        // widths: max(4,5,13)=13, max(7,3,6)=7; each cell is right-padded to its width
        assert_eq!(lines[2], "  short          1.0    ");
        assert_eq!(lines[3], "  a-longer-name  10.0.0 ");
    }

    #[test]
    fn render_trailing_unpadded_column() {
        let t = make_table(vec!["Name", "Installed"], 2, vec![vec!["my-agent", "1.0.0", "ok"]]);
        let out = t.render();
        let lines: Vec<&str> = out.lines().collect();
        // header + separator + 1 data row = 3
        assert_eq!(lines.len(), 3);
        // "my-agent" padded to 8, "1.0.0" padded to 9 (= "Installed" len), then "ok" unpadded
        assert_eq!(lines[2], "  my-agent  1.0.0      ok");
    }

    #[test]
    fn render_empty_rows_produces_only_header_and_separator() {
        let t = make_table(vec!["Name", "Status"], 2, vec![]);
        let out = t.render();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
    }
}
