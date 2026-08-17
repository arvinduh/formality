pub mod render;

pub use render::{
  Table, max_line_display_width, render, render_json, separator_for_content,
  separator_line, strip_ansi_escapes,
};

use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use unicode_width::UnicodeWidthStr;

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

#[cfg(test)]
mod tests;
