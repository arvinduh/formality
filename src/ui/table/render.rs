//! Table rendering: the `Table` builder and the comfy-table-backed renderer.

use super::{
  Align, Cell, Column, Layout, Overflow, Palette, Row, RowKind, Span, Style,
  WidthPolicy,
};
use serde::{Deserialize, Serialize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The top-level table specification.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Table {
  /// Column definitions.
  pub columns: Vec<Column>,
  /// Row contents.
  pub rows: Vec<Row>,
  /// Layout and width options.
  #[serde(default)]
  pub layout: Layout,
}

impl Table {
  /// Creates a new [`Table`] with specified columns.
  #[must_use]
  pub fn new(columns: Vec<Column>) -> Self {
    Self {
      columns,
      rows: Vec::new(),
      layout: Layout::default(),
    }
  }

  /// Adds a row to the table in place.
  pub fn add_row(&mut self, row: Row) -> &mut Self {
    self.rows.push(row);
    self
  }

  /// Sets the layout policy for the table.
  #[must_use]
  pub fn layout(mut self, layout: Layout) -> Self {
    self.layout = layout;
    self
  }

  /// Renders the table to a terminal string using the given color palette.
  #[must_use]
  pub fn render(&self, palette: &Palette) -> String {
    render(self, palette)
  }
}

fn take_prefix_by_width(text: &str, budget: usize) -> (String, usize) {
  let mut prefix = String::new();
  let mut width = 0;
  for ch in text.chars() {
    let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
    if width + ch_w > budget {
      break;
    }
    prefix.push(ch);
    width += ch_w;
  }
  (prefix, width)
}

pub(super) fn truncate_spans(
  spans: &[Span],
  max_width: usize,
  suffix: &str,
) -> Vec<Span> {
  let suffix_width = suffix.width();
  if suffix_width >= max_width {
    let (truncated_suffix, _) = take_prefix_by_width(suffix, max_width);
    return vec![Span::plain(truncated_suffix)];
  }
  let target_width = max_width - suffix_width;
  let mut current_width = 0;
  let mut result = Vec::new();

  for span in spans {
    let span_width = span.display_width();
    if current_width + span_width <= target_width {
      result.push(span.clone());
      current_width += span_width;
    } else {
      let (partial, _) = take_prefix_by_width(
        &span.text,
        target_width.saturating_sub(current_width),
      );
      if !partial.is_empty() {
        result.push(Span::new(partial, span.style));
      }
      result.push(Span::plain(suffix));
      return result;
    }
  }

  result.push(Span::plain(suffix));
  result
}

fn clip_spans(spans: &[Span], max_width: usize) -> Vec<Span> {
  let mut current_width = 0;
  let mut result = Vec::new();

  for span in spans {
    let span_width = span.display_width();
    if current_width + span_width <= max_width {
      result.push(span.clone());
      current_width += span_width;
    } else {
      let (partial, _) = take_prefix_by_width(
        &span.text,
        max_width.saturating_sub(current_width),
      );
      if !partial.is_empty() {
        result.push(Span::new(partial, span.style));
      }
      return result;
    }
  }
  result
}

fn render_cell_to_string(
  cell: &Cell,
  col_overflow: &Overflow,
  max_width_opt: Option<usize>,
  palette: &Palette,
) -> String {
  let overflow = cell.overflow.as_ref().unwrap_or(col_overflow);
  let mut buf = String::new();
  if let Some(max_w) = max_width_opt
    && cell.display_width() > max_w
  {
    match overflow {
      Overflow::Clip => {
        for span in clip_spans(&cell.spans, max_w) {
          buf.push_str(&palette.apply(&span.text, span.style));
        }
        return buf;
      }
      Overflow::Truncate { suffix } => {
        for span in truncate_spans(&cell.spans, max_w, suffix) {
          buf.push_str(&palette.apply(&span.text, span.style));
        }
        return buf;
      }
      Overflow::Wrap => {}
    }
  }

  for span in &cell.spans {
    buf.push_str(&palette.apply(&span.text, span.style));
  }
  buf
}

fn to_comfy_align(align: Align) -> comfy_table::CellAlignment {
  match align {
    Align::Left => comfy_table::CellAlignment::Left,
    Align::Center => comfy_table::CellAlignment::Center,
    Align::Right => comfy_table::CellAlignment::Right,
  }
}

/// Render a semantic Table specification into a formatted string using comfy-table.
// Renders rich formatted tables with palette coloring, column width constraints, row spanning, and terminal clamping.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn render(spec: &Table, palette: &Palette) -> String {
  let mut table = comfy_table::Table::new();
  table.load_style(comfy_table::presets::NOTHING.header_separator(
    comfy_table::LineStyle::new('\u{2500}', '\u{2500}', '\u{2500}', '\u{2500}'),
  ));
  table.style_text_only();
  table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

  // Terminal width / max_width handling
  let mut target_width = spec.layout.max_width;
  if spec.layout.clamp_to_terminal {
    let term_width = detect_terminal_width();
    if term_width < target_width {
      target_width = term_width;
    }
  }
  let table_width = target_width.saturating_sub(spec.layout.indent);
  table.set_width(table_width);

  let num_cols = spec.columns.len();
  let has_headers = spec
    .columns
    .iter()
    .any(|c| c.header.spans.iter().any(|s| !s.text.is_empty()));

  if has_headers {
    let mut header_row = comfy_table::Row::new();
    for col in &spec.columns {
      let content =
        render_cell_to_string(&col.header, &col.overflow, None, palette);
      let cell_align = col.header.align.unwrap_or(col.align);
      let cell = comfy_table::Cell::new(content)
        .set_alignment(to_comfy_align(cell_align));
      header_row.add_cell(cell);
    }
    table.set_header(header_row);
  }

  let padding_w = (spec.layout.padding.0 + spec.layout.padding.1) as usize;
  let col_max_widths: Vec<Option<usize>> = spec
    .columns
    .iter()
    .map(|col| match col.width {
      WidthPolicy::Fixed(w) | WidthPolicy::Max(w) => {
        Some((w as usize).saturating_sub(padding_w))
      }
      WidthPolicy::Range(_, max) => {
        Some((max as usize).saturating_sub(padding_w))
      }
      WidthPolicy::Min(w) => Some((w as usize).saturating_sub(padding_w)),
      WidthPolicy::Pct(pct) => Some(
        ((table_width as usize * pct as usize) / 100).saturating_sub(padding_w),
      ),
      WidthPolicy::Auto => {
        Some((table_width as usize).saturating_sub(padding_w))
      }
    })
    .collect();

  let mut group_titles: Vec<String> = Vec::new();

  // Populate data rows
  let mut row_iter = spec.rows.iter().peekable();
  while let Some(row) = row_iter.next() {
    match &row.kind {
      RowKind::Data => {
        let mut comfy_row = comfy_table::Row::new();
        for i in 0..num_cols {
          let cell = row.cells.get(i);
          let col = spec.columns.get(i);
          let col_overflow = col.map_or(&Overflow::Wrap, |c| &c.overflow);
          let col_align = col.map_or(Align::Left, |c| c.align);
          let max_w = col_max_widths.get(i).copied().flatten();

          let (content, align) = if let Some(c) = cell {
            (
              render_cell_to_string(c, col_overflow, max_w, palette),
              c.align.unwrap_or(col_align),
            )
          } else {
            (String::new(), col_align)
          };

          let comfy_cell = comfy_table::Cell::new(content)
            .set_alignment(to_comfy_align(align));
          comfy_row.add_cell(comfy_cell);
        }

        if let Some(max_h) = row.max_height {
          comfy_row.max_height(max_h);
        }

        table.add_row(comfy_row);

        // Comfortable density adds an empty spacer line after data rows
        if spec.layout.density == super::Density::Comfortable
          && row_iter
            .peek()
            .is_some_and(|next| matches!(next.kind, RowKind::Data))
        {
          let mut blank_row = comfy_table::Row::new();
          for _ in 0..num_cols {
            blank_row.add_cell(comfy_table::Cell::new(""));
          }
          table.add_row(blank_row);
        }
      }
      RowKind::Blank => {
        let mut comfy_row = comfy_table::Row::new();
        for _ in 0..num_cols {
          comfy_row.add_cell(comfy_table::Cell::new(""));
        }
        table.add_row(comfy_row);
      }
      RowKind::Rule => {
        let mut comfy_row = comfy_table::Row::new();
        for _ in 0..num_cols {
          comfy_row.add_cell(comfy_table::Cell::new("\u{2500}"));
        }
        table.add_row(comfy_row);
      }
      RowKind::Group(title) => {
        let mut comfy_row = comfy_table::Row::new();
        let idx = group_titles.len();
        group_titles.push(palette.apply(title, Style::Strong));
        comfy_row.add_cell(comfy_table::Cell::new(format!("_G{idx}_")));
        for _ in 1..num_cols {
          comfy_row.add_cell(comfy_table::Cell::new(""));
        }
        table.add_row(comfy_row);
      }
    }
  }

  // Apply column constraints & padding
  for (i, col) in spec.columns.iter().enumerate() {
    if let Some(comfy_col) = table.column_mut(i) {
      comfy_col.set_padding((spec.layout.padding.0, spec.layout.padding.1));
      comfy_col.set_cell_alignment(to_comfy_align(col.align));

      match col.width {
        WidthPolicy::Auto => {}
        WidthPolicy::Fixed(w) => {
          comfy_col.set_constraint(comfy_table::ColumnConstraint::Absolute(
            comfy_table::Width::Fixed(w),
          ));
        }
        WidthPolicy::Min(w) => {
          comfy_col.set_constraint(
            comfy_table::ColumnConstraint::LowerBoundary(
              comfy_table::Width::Fixed(w),
            ),
          );
        }
        WidthPolicy::Max(w) => {
          comfy_col.set_constraint(
            comfy_table::ColumnConstraint::UpperBoundary(
              comfy_table::Width::Fixed(w),
            ),
          );
        }
        WidthPolicy::Range(min, max) => {
          comfy_col.set_constraint(comfy_table::ColumnConstraint::Boundaries {
            lower: comfy_table::Width::Fixed(min),
            upper: comfy_table::Width::Fixed(max),
          });
        }
        WidthPolicy::Pct(pct) => {
          comfy_col.set_constraint(comfy_table::ColumnConstraint::Absolute(
            comfy_table::Width::Percentage(u16::from(pct)),
          ));
        }
      }
    }
  }

  let formatted = table.trim_fmt();

  // If there are rule rows (which comfy-table may render as spaces separated by '─'),
  // post-process rule rows so they form a continuous unbroken rule line across the full table width.
  let table_width = max_line_display_width(&formatted);
  let processed = if table_width > 0 {
    formatted
      .lines()
      .map(|line| {
        let stripped = strip_ansi_escapes(line);
        if let Some(g_idx) =
          (0..group_titles.len()).find(|&i| line.contains(&format!("_G{i}_")))
        {
          group_titles[g_idx].clone()
        } else if !stripped.is_empty()
          && stripped.chars().all(|c| c == '\u{2500}' || c == ' ')
        {
          "\u{2500}".repeat(table_width)
        } else {
          line.to_string()
        }
      })
      .collect::<Vec<_>>()
      .join("\n")
  } else {
    formatted
  };

  if spec.layout.indent > 0 {
    let indent_str = " ".repeat(spec.layout.indent as usize);
    processed
      .lines()
      .map(|line| {
        if line.trim().is_empty() {
          String::new()
        } else {
          format!("{indent_str}{line}")
        }
      })
      .collect::<Vec<_>>()
      .join("\n")
  } else {
    processed
  }
}

/// Strips ANSI CSI and OSC escape sequences from a string.
#[must_use]
pub fn strip_ansi_escapes(s: &str) -> String {
  #[derive(Copy, Clone, PartialEq, Eq)]
  enum AnsiState {
    Normal,
    Esc,
    Csi,
    Osc,
    OscEsc,
    EscIntermediate,
  }

  let mut result = String::with_capacity(s.len());
  let mut state = AnsiState::Normal;

  for c in s.chars() {
    match state {
      AnsiState::Normal => {
        if c == '\x1b' {
          state = AnsiState::Esc;
        } else {
          result.push(c);
        }
      }
      AnsiState::Esc => match c {
        '[' => state = AnsiState::Csi,
        ']' => state = AnsiState::Osc,
        '\x1b' => {
          state = AnsiState::Esc;
        }
        '\x20'..='\x2f' => {
          state = AnsiState::EscIntermediate;
        }
        '\x30'..='\x7e' => {
          state = AnsiState::Normal;
        }
        _ => {
          state = AnsiState::Normal;
          result.push(c);
        }
      },
      AnsiState::Csi => {
        match c {
          '\x1b' => {
            state = AnsiState::Esc;
          }
          '\x20'..='\x3f' => {
            // Parameter or intermediate byte; remain in CSI
          }
          '\x40'..='\x7e' => {
            // Final byte terminating CSI sequence
            state = AnsiState::Normal;
          }
          _ => {
            state = AnsiState::Normal;
            result.push(c);
          }
        }
      }
      AnsiState::Osc => match c {
        '\x07' => {
          state = AnsiState::Normal;
        }
        '\x1b' => {
          state = AnsiState::OscEsc;
        }
        '\n' | '\r' => {
          state = AnsiState::Normal;
          result.push(c);
        }
        _ => {}
      },
      AnsiState::OscEsc => match c {
        '\\' => {
          state = AnsiState::Normal;
        }
        '[' => {
          state = AnsiState::Csi;
        }
        ']' => {
          state = AnsiState::Osc;
        }
        '\x1b' => {}
        _ => {
          state = AnsiState::Normal;
          result.push(c);
        }
      },
      AnsiState::EscIntermediate => match c {
        '\x1b' => {
          state = AnsiState::Esc;
        }
        '\x20'..='\x2f' => {}
        '\x30'..='\x7e' => {
          state = AnsiState::Normal;
        }
        _ => {
          state = AnsiState::Normal;
          result.push(c);
        }
      },
    }
  }

  result
}

/// Measures the maximum visual character display width across all lines in a string,
/// ignoring ANSI escape codes and accounting for multi-byte/CJK Unicode width.
#[must_use]
pub fn max_line_display_width(s: &str) -> usize {
  s.lines()
    .map(|line| {
      let stripped = strip_ansi_escapes(line);
      unicode_width::UnicodeWidthStr::width(stripped.as_str())
    })
    .max()
    .unwrap_or(0)
}

/// Detects the active terminal column width (clamped to [40, 160]), defaulting to 80 when stdout is not a TTY.
#[must_use]
pub fn detect_terminal_width() -> u16 {
  use std::io::IsTerminal;
  if std::io::stdout().is_terminal()
    && let Ok((w, _)) = crossterm::terminal::size()
    && w > 0
  {
    return w.clamp(40, 160);
  }
  80
}

/// Returns a horizontal rule line of `─` matching the specified width.
#[must_use]
pub fn separator_line(width: usize) -> String {
  let w = if width == 0 {
    detect_terminal_width() as usize
  } else {
    width
  };
  "\u{2500}".repeat(w)
}

/// Returns a horizontal rule line of `─` matching the maximum visual width of the provided content.
#[must_use]
pub fn separator_for_content(content: &str) -> String {
  let w = max_line_display_width(content);
  let final_w = if w > 0 {
    w
  } else {
    detect_terminal_width() as usize
  };
  separator_line(final_w)
}

/// Render a JSON-encoded table specification directly into a formatted string.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] if parsing `spec_json` fails.
pub fn render_json(spec_json: &str) -> Result<String, serde_json::Error> {
  let table: Table = serde_json::from_str(spec_json)?;
  let palette = Palette::detect();
  Ok(render(&table, &palette))
}
