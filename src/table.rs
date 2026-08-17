use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Semantic style definitions for terminal text rendering.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum Style {
  #[default]
  Plain,
  Dim,
  Strong,
  Path,
  Tool,
  Ok,
  Warn,
  Error,
  Info,
}

/// A styled segment of text.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Span {
  pub text: String,
  #[serde(default)]
  pub style: Style,
}

impl Span {
  pub fn new(text: impl Into<String>, style: Style) -> Self {
    Self {
      text: text.into(),
      style,
    }
  }

  pub fn plain(text: impl Into<String>) -> Self {
    Self::new(text, Style::Plain)
  }

  pub fn styled(text: impl Into<String>, style: Style) -> Self {
    Self::new(text, style)
  }

  pub fn display_width(&self) -> usize {
    self.text.as_str().width()
  }
}

impl From<&str> for Span {
  fn from(s: &str) -> Self {
    Span::plain(s)
  }
}

impl From<String> for Span {
  fn from(s: String) -> Self {
    Span::plain(s)
  }
}

/// Text alignment within a column or cell.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Align {
  #[default]
  Left,
  Center,
  Right,
}

/// Overflow handling policy when content exceeds column bounds.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
  #[default]
  Wrap,
  Truncate {
    #[serde(default = "default_truncate_suffix")]
    suffix: String,
  },
  Clip,
}

fn default_truncate_suffix() -> String {
  "...".to_string()
}

impl Overflow {
  pub fn truncate(suffix: impl Into<String>) -> Self {
    Overflow::Truncate {
      suffix: suffix.into(),
    }
  }

  pub fn default_truncate() -> Self {
    Overflow::Truncate {
      suffix: default_truncate_suffix(),
    }
  }
}

/// A single cell inside a table row, composed of semantic spans.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Cell {
  pub spans: Vec<Span>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub align: Option<Align>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub overflow: Option<Overflow>,
}

impl Cell {
  pub fn new(spans: Vec<Span>) -> Self {
    Self {
      spans,
      align: None,
      overflow: None,
    }
  }

  pub fn text(text: impl Into<String>) -> Self {
    Self {
      spans: vec![Span::plain(text)],
      align: None,
      overflow: None,
    }
  }

  pub fn span(span: Span) -> Self {
    Self {
      spans: vec![span],
      align: None,
      overflow: None,
    }
  }

  pub fn styled(text: impl Into<String>, style: Style) -> Self {
    Self {
      spans: vec![Span::styled(text, style)],
      align: None,
      overflow: None,
    }
  }

  pub fn display_width(&self) -> usize {
    self.spans.iter().map(|s| s.display_width()).sum()
  }

  pub fn align(mut self, align: Align) -> Self {
    self.align = Some(align);
    self
  }

  pub fn overflow(mut self, overflow: Overflow) -> Self {
    self.overflow = Some(overflow);
    self
  }

  pub fn push(&mut self, span: Span) {
    self.spans.push(span);
  }

  pub fn with_span(mut self, span: Span) -> Self {
    self.spans.push(span);
    self
  }
}

impl From<&str> for Cell {
  fn from(s: &str) -> Self {
    Cell::text(s)
  }
}

impl From<String> for Cell {
  fn from(s: String) -> Self {
    Cell::text(s)
  }
}

impl From<Span> for Cell {
  fn from(s: Span) -> Self {
    Cell::span(s)
  }
}

/// Color rendering mode for terminal palettes.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum PaletteMode {
  None,
  #[default]
  Ansi16,
  Truecolor,
}

/// Semantic palette that maps `Style` to ANSI escape codes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
  pub mode: PaletteMode,
}

impl Palette {
  pub fn new(mode: PaletteMode) -> Self {
    Self { mode }
  }

  pub fn none() -> Self {
    Self {
      mode: PaletteMode::None,
    }
  }

  pub fn ansi16() -> Self {
    Self {
      mode: PaletteMode::Ansi16,
    }
  }

  pub fn truecolor() -> Self {
    Self {
      mode: PaletteMode::Truecolor,
    }
  }

  pub fn mode(&self) -> PaletteMode {
    self.mode
  }

  /// Automatically detect the terminal color capability, respecting standard
  /// environment variables (NO_COLOR, FORCE_COLOR, CLICOLOR_FORCE, COLORTERM, TERM).
  pub fn detect() -> Self {
    // 1. Respect NO_COLOR if set and non-empty
    if std::env::var("NO_COLOR").is_ok_and(|val| !val.is_empty()) {
      return Self::none();
    }

    // 2. Forced color overrides
    let force_color = std::env::var("FORCE_COLOR").is_ok()
      || std::env::var("CLICOLOR_FORCE").is_ok()
      || std::env::var("GITHUB_ACTIONS").is_ok();

    if !force_color && !std::io::stdout().is_terminal() {
      return Self::none();
    }

    if std::env::var("TERM").is_ok_and(|term| term == "dumb" && !force_color) {
      return Self::none();
    }

    if std::env::var("COLORTERM").is_ok_and(|ct| {
      ct.eq_ignore_ascii_case("truecolor") || ct.eq_ignore_ascii_case("24bit")
    }) {
      return Self::truecolor();
    }

    Self::ansi16()
  }

  /// Get SGR opening and closing escapes for a given Style.
  pub fn style_sgr(&self, style: Style) -> (&'static str, &'static str) {
    match self.mode {
      PaletteMode::None => ("", ""),
      PaletteMode::Ansi16 => match style {
        Style::Plain => ("", ""),
        Style::Dim => ("\x1b[2m", "\x1b[0m"),
        Style::Strong => ("\x1b[1m", "\x1b[0m"),
        Style::Path => ("\x1b[36m", "\x1b[0m"),
        Style::Tool => ("\x1b[1;35m", "\x1b[0m"),
        Style::Ok => ("\x1b[1;32m", "\x1b[0m"),
        Style::Warn => ("\x1b[1;33m", "\x1b[0m"),
        Style::Error => ("\x1b[1;31m", "\x1b[0m"),
        Style::Info => ("\x1b[34m", "\x1b[0m"),
      },
      PaletteMode::Truecolor => match style {
        Style::Plain => ("", ""),
        Style::Dim => ("\x1b[2m", "\x1b[0m"),
        Style::Strong => ("\x1b[1m", "\x1b[0m"),
        Style::Path => ("\x1b[38;2;100;180;240m", "\x1b[0m"),
        Style::Tool => ("\x1b[1;38;2;200;120;255m", "\x1b[0m"),
        Style::Ok => ("\x1b[1;38;2;80;200;120m", "\x1b[0m"),
        Style::Warn => ("\x1b[1;38;2;240;180;50m", "\x1b[0m"),
        Style::Error => ("\x1b[1;38;2;240;80;80m", "\x1b[0m"),
        Style::Info => ("\x1b[38;2;80;150;240m", "\x1b[0m"),
      },
    }
  }

  /// Apply style escape codes to a text slice.
  pub fn apply(&self, text: &str, style: Style) -> String {
    if text.is_empty() {
      return String::new();
    }
    let (prefix, suffix) = self.style_sgr(style);
    if prefix.is_empty() && suffix.is_empty() {
      text.to_string()
    } else {
      format!("{}{}{}", prefix, text, suffix)
    }
  }
}

/// Width policy for a table column.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum WidthPolicy {
  #[default]
  Auto,
  Fixed(u16),
  Min(u16),
  Max(u16),
  Range(u16, u16),
  Pct(u8),
}

/// Semantic kind of a table row.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
  #[default]
  Data,
  Rule,
  Blank,
  Group(String),
}

/// Column configuration including header, alignment, width policy, and overflow rule.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Column {
  pub header: Cell,
  #[serde(default)]
  pub align: Align,
  #[serde(default)]
  pub width: WidthPolicy,
  #[serde(default)]
  pub overflow: Overflow,
  #[serde(default)]
  pub priority: u8,
}

impl Column {
  pub fn new(header: impl Into<Cell>) -> Self {
    Self {
      header: header.into(),
      align: Align::Left,
      width: WidthPolicy::Auto,
      overflow: Overflow::Wrap,
      priority: 0,
    }
  }

  pub fn align(mut self, align: Align) -> Self {
    self.align = align;
    self
  }

  pub fn width(mut self, width: WidthPolicy) -> Self {
    self.width = width;
    self
  }

  pub fn overflow(mut self, overflow: Overflow) -> Self {
    self.overflow = overflow;
    self
  }

  pub fn priority(mut self, priority: u8) -> Self {
    self.priority = priority;
    self
  }
}

/// A row in the table containing cells and rendering metadata.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Row {
  pub cells: Vec<Cell>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub max_height: Option<usize>,
  #[serde(default)]
  pub kind: RowKind,
}

impl Row {
  pub fn new(cells: Vec<Cell>) -> Self {
    Self {
      cells,
      max_height: None,
      kind: RowKind::Data,
    }
  }

  pub fn data(cells: Vec<Cell>) -> Self {
    Self::new(cells)
  }

  pub fn rule() -> Self {
    Self {
      cells: Vec::new(),
      max_height: None,
      kind: RowKind::Rule,
    }
  }

  pub fn blank() -> Self {
    Self {
      cells: Vec::new(),
      max_height: None,
      kind: RowKind::Blank,
    }
  }

  pub fn group(title: impl Into<String>) -> Self {
    Self {
      cells: Vec::new(),
      max_height: None,
      kind: RowKind::Group(title.into()),
    }
  }

  pub fn max_height(mut self, height: usize) -> Self {
    self.max_height = Some(height);
    self
  }
}

/// Table density mode.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Density {
  #[default]
  Compact,
  Comfortable,
}

/// Geometry and layout settings for table rendering.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Layout {
  #[serde(default = "default_max_width")]
  pub max_width: u16,
  #[serde(default = "default_clamp_to_terminal")]
  pub clamp_to_terminal: bool,
  #[serde(default = "default_padding")]
  pub padding: (u16, u16),
  #[serde(default)]
  pub density: Density,
  #[serde(default)]
  pub indent: u16,
}

fn default_max_width() -> u16 {
  100
}

fn default_clamp_to_terminal() -> bool {
  true
}

fn default_padding() -> (u16, u16) {
  (1, 1)
}

impl Default for Layout {
  fn default() -> Self {
    Self {
      max_width: default_max_width(),
      clamp_to_terminal: default_clamp_to_terminal(),
      padding: default_padding(),
      density: Density::Compact,
      indent: 0,
    }
  }
}

impl Layout {
  pub fn compact() -> Self {
    Self {
      density: Density::Compact,
      padding: (1, 1),
      ..Default::default()
    }
  }

  pub fn comfortable() -> Self {
    Self {
      density: Density::Comfortable,
      padding: (1, 1),
      ..Default::default()
    }
  }

  pub fn max_width(mut self, width: u16) -> Self {
    self.max_width = width;
    self
  }

  pub fn indent(mut self, indent: u16) -> Self {
    self.indent = indent;
    self
  }

  pub fn padding(mut self, left: u16, right: u16) -> Self {
    self.padding = (left, right);
    self
  }

  pub fn clamp_to_terminal(mut self, clamp: bool) -> Self {
    self.clamp_to_terminal = clamp;
    self
  }
}

/// The top-level table specification.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Table {
  pub columns: Vec<Column>,
  pub rows: Vec<Row>,
  #[serde(default)]
  pub layout: Layout,
}

impl Table {
  pub fn new(columns: Vec<Column>) -> Self {
    Self {
      columns,
      rows: Vec::new(),
      layout: Layout::default(),
    }
  }

  pub fn empty() -> Self {
    Self::default()
  }

  pub fn add_row(&mut self, row: Row) -> &mut Self {
    self.rows.push(row);
    self
  }

  pub fn add_rows(&mut self, rows: impl IntoIterator<Item = Row>) -> &mut Self {
    self.rows.extend(rows);
    self
  }

  pub fn with_row(mut self, row: Row) -> Self {
    self.rows.push(row);
    self
  }

  pub fn with_rows(mut self, rows: impl IntoIterator<Item = Row>) -> Self {
    self.rows.extend(rows);
    self
  }

  pub fn layout(mut self, layout: Layout) -> Self {
    self.layout = layout;
    self
  }

  pub fn render(&self, palette: &Palette) -> String {
    render(self, palette)
  }
}

fn truncate_spans(spans: &[Span], max_width: usize, suffix: &str) -> Vec<Span> {
  let suffix_width = suffix.width();
  if suffix_width >= max_width {
    return vec![Span::plain(
      suffix.chars().take(max_width).collect::<String>(),
    )];
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
      let mut partial = String::new();
      for ch in span.text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > target_width {
          break;
        }
        partial.push(ch);
        current_width += ch_width;
      }
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
      let mut partial = String::new();
      for ch in span.text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > max_width {
          break;
        }
        partial.push(ch);
        current_width += ch_width;
      }
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
  let spans = if let Some(max_w) = max_width_opt {
    if cell.display_width() > max_w {
      match overflow {
        Overflow::Clip => clip_spans(&cell.spans, max_w),
        Overflow::Truncate { suffix } => {
          truncate_spans(&cell.spans, max_w, suffix)
        }
        Overflow::Wrap => cell.spans.clone(),
      }
    } else {
      cell.spans.clone()
    }
  } else {
    cell.spans.clone()
  };

  let mut buf = String::new();
  for span in spans {
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
pub fn render(spec: &Table, palette: &Palette) -> String {
  let mut table = comfy_table::Table::new();
  table.load_style(comfy_table::presets::NOTHING.header_separator(
    comfy_table::LineStyle::new('\u{2500}', '\u{2500}', '\u{2500}', '\u{2500}'),
  ));
  table.style_text_only();
  table.set_content_arrangement(comfy_table::ContentArrangement::Dynamic);

  // Terminal width / max_width handling
  let mut target_width = spec.layout.max_width;
  if spec.layout.clamp_to_terminal
    && let Ok((w, _)) = crossterm::terminal::size()
    && w > 0
    && w < target_width
  {
    target_width = w;
  }
  table.set_width(target_width);

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

  // Populate data rows
  for row in &spec.rows {
    match &row.kind {
      RowKind::Data => {
        let mut comfy_row = comfy_table::Row::new();
        for i in 0..num_cols {
          let cell = row.cells.get(i);
          let col = spec.columns.get(i);
          let col_overflow =
            col.map(|c| &c.overflow).unwrap_or(&Overflow::Wrap);
          let col_align = col.map(|c| c.align).unwrap_or(Align::Left);

          let (content, align) = if let Some(c) = cell {
            let max_w = if let Some(col_spec) = col {
              let padding_w =
                (spec.layout.padding.0 + spec.layout.padding.1) as usize;
              match col_spec.width {
                WidthPolicy::Fixed(w) => {
                  Some((w as usize).saturating_sub(padding_w))
                }
                WidthPolicy::Max(w) => {
                  Some((w as usize).saturating_sub(padding_w))
                }
                WidthPolicy::Range(_, max) => {
                  Some((max as usize).saturating_sub(padding_w))
                }
                _ => None,
              }
            } else {
              None
            };
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
        let formatted_title = palette.apply(title, Style::Strong);
        comfy_row.add_cell(comfy_table::Cell::new(formatted_title));
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
            comfy_table::Width::Percentage(pct as u16),
          ));
        }
      }
    }
  }

  let formatted = table.trim_fmt();

  if spec.layout.indent > 0 {
    let indent_str = " ".repeat(spec.layout.indent as usize);
    formatted
      .lines()
      .map(|line| {
        if line.trim().is_empty() {
          String::new()
        } else {
          format!("{}{}", indent_str, line)
        }
      })
      .collect::<Vec<_>>()
      .join("\n")
  } else {
    formatted
  }
}

/// Render a JSON-encoded table specification directly into a formatted string.
pub fn render_json(spec_json: &str) -> Result<String, serde_json::Error> {
  let table: Table = serde_json::from_str(spec_json)?;
  let palette = Palette::detect();
  Ok(render(&table, &palette))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_span_width_calculation_unicode_cjk() {
    let ascii_span = Span::plain("hello");
    assert_eq!(ascii_span.display_width(), 5);

    // CJK characters have display width 2 each
    let cjk_span = Span::plain("\u{4f60}\u{597d}\u{4e16}\u{754c}");
    assert_eq!(cjk_span.display_width(), 8);

    // Mixed ASCII, emoji, and CJK
    let mixed_span = Span::plain("Rust \u{1f980} \u{7f16}\u{7a0b}");
    // "Rust " = 5, emoji = 2, " " = 1, Chinese = 4 => 12
    assert_eq!(mixed_span.display_width(), 12);

    let cell = Cell::new(vec![
      Span::styled("Status: ", Style::Strong),
      Span::styled("\u{6210}\u{529f}", Style::Ok),
      Span::plain(" (OK)"),
    ]);
    // "Status: " = 8, Chinese = 4, " (OK)" = 5 => 17
    assert_eq!(cell.display_width(), 17);
  }

  #[test]
  fn test_palette_styling_none_vs_ansi16_vs_truecolor() {
    let none_pal = Palette::none();
    assert_eq!(none_pal.mode(), PaletteMode::None);

    let styles = [
      Style::Plain,
      Style::Dim,
      Style::Strong,
      Style::Path,
      Style::Tool,
      Style::Ok,
      Style::Warn,
      Style::Error,
      Style::Info,
    ];

    for s in styles {
      let applied = none_pal.apply("text", s);
      assert_eq!(
        applied, "text",
        "Style {:?} should have no SGR in None mode",
        s
      );
    }

    let ansi_pal = Palette::ansi16();
    assert_eq!(ansi_pal.mode(), PaletteMode::Ansi16);
    let ok_ansi = ansi_pal.apply("PASS", Style::Ok);
    assert!(ok_ansi.starts_with("\x1b[1;32m"));
    assert!(ok_ansi.ends_with("\x1b[0m"));

    let err_ansi = ansi_pal.apply("FAIL", Style::Error);
    assert!(err_ansi.starts_with("\x1b[1;31m"));

    let tc_pal = Palette::truecolor();
    assert_eq!(tc_pal.mode(), PaletteMode::Truecolor);
    let ok_tc = tc_pal.apply("PASS", Style::Ok);
    assert!(ok_tc.contains("38;2;80;200;120m"));
    assert!(ok_tc.ends_with("\x1b[0m"));

    let path_tc = tc_pal.apply("src/table.rs", Style::Path);
    assert!(path_tc.contains("38;2;100;180;240m"));
  }

  #[test]
  fn test_wrapping_and_multiline_spans() {
    let mut table = Table::new(vec![
      Column::new("Col 1").width(WidthPolicy::Fixed(12)).overflow(
        Overflow::Truncate {
          suffix: "...".to_string(),
        },
      ),
      Column::new("Col 2")
        .width(WidthPolicy::Fixed(12))
        .overflow(Overflow::Clip),
      Column::new("Col 3")
        .width(WidthPolicy::Fixed(16))
        .overflow(Overflow::Wrap),
    ]);

    table.add_row(Row::new(vec![
      Cell::text("A very long string that needs truncation"),
      Cell::text("A very long string that needs clipping"),
      Cell::text("Word wrap test"),
    ]));

    table.add_row(Row::blank());
    table.add_row(Row::rule());
    table.add_row(Row::group("Summary"));

    let pal = Palette::none();
    let rendered = render(&table, &pal);

    assert!(rendered.contains("Col 1"));
    assert!(rendered.contains("Col 2"));
    assert!(rendered.contains("Col 3"));
    assert!(rendered.contains("..."));
    assert!(rendered.contains("Summary"));
    assert!(rendered.contains('\u{2500}'));
  }

  #[test]
  fn test_table_serialization_deserialization_json() {
    let table = Table::new(vec![
      Column::new("Surface")
        .align(Align::Left)
        .width(WidthPolicy::Fixed(12)),
      Column::new("Status")
        .align(Align::Center)
        .width(WidthPolicy::Fixed(10)),
      Column::new("Count")
        .align(Align::Right)
        .width(WidthPolicy::Fixed(8)),
    ])
    .layout(Layout::compact().indent(2))
    .with_row(Row::new(vec![
      Cell::styled("rust", Style::Tool),
      Cell::styled("PASS", Style::Ok),
      Cell::text("42"),
    ]))
    .with_row(Row::rule())
    .with_row(Row::group("Totals"))
    .with_row(Row::new(vec![
      Cell::text("Total"),
      Cell::text(""),
      Cell::styled("42", Style::Strong),
    ]));

    let json_str = serde_json::to_string_pretty(&table)
      .expect("Failed to serialize table to JSON");
    let deserialized: Table = serde_json::from_str(&json_str)
      .expect("Failed to deserialize table from JSON");

    assert_eq!(table, deserialized);

    let rendered_direct = render(&table, &Palette::none());
    let rendered_json =
      render_json(&json_str).expect("Failed to render json table");
    // With detect or none mode, the layout structure matches
    assert!(rendered_json.contains("Surface"));
    assert!(rendered_json.contains("rust"));
    assert!(rendered_json.contains("42"));
    assert_eq!(
      rendered_direct.lines().count(),
      rendered_json.lines().count()
    );
  }

  #[test]
  fn test_doctor_table_rendering_consistency() {
    let mut table = Table::new(vec![
      Column::new("Status").width(WidthPolicy::Fixed(10)),
      Column::new("Tool").width(WidthPolicy::Fixed(14)),
      Column::new("Surface").width(WidthPolicy::Fixed(10)),
      Column::new("Details").width(WidthPolicy::Auto),
    ])
    .layout(Layout::compact().indent(2));

    table.add_row(Row::new(vec![
      Cell::styled("[READY]", Style::Ok),
      Cell::styled("rustfmt", Style::Tool),
      Cell::styled("rust", Style::Dim),
      Cell::new(vec![
        Span::styled("/usr/bin/rustfmt", Style::Dim),
        Span::styled(" (v1.8.0)", Style::Info),
      ]),
    ]));

    table.add_row(Row::new(vec![
      Cell::styled("[WARN] ", Style::Warn),
      Cell::styled("clippy", Style::Warn),
      Cell::styled("rust", Style::Dim),
      Cell::styled(" (v1.70.0 < MSTV v1.75.0)", Style::Warn),
    ]));

    table.add_row(Row::new(vec![
      Cell::styled("[MISS] ", Style::Warn),
      Cell::styled("ruff", Style::Warn),
      Cell::styled("python", Style::Dim),
      Cell::styled(
        "An extremely fast Python linter and code formatter",
        Style::Dim,
      ),
    ]));

    let pal_none = Palette::none();
    let rendered_none = render(&table, &pal_none);

    assert!(rendered_none.contains("[READY]"));
    assert!(rendered_none.contains("rustfmt"));
    assert!(rendered_none.contains("[WARN]"));
    assert!(rendered_none.contains("clippy"));
    assert!(rendered_none.contains("[MISS]"));
    assert!(rendered_none.contains("ruff"));
    assert!(!rendered_none.contains("\x1b["));

    let pal_tc = Palette::truecolor();
    let rendered_tc = render(&table, &pal_tc);
    assert!(rendered_tc.contains("\x1b["));
    assert!(rendered_tc.contains("38;2;80;200;120m")); // Ok color
  }
}
