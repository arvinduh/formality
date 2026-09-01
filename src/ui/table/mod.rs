//! Table data model ([`Table`], [`Row`], [`Cell`], [`Column`]) and the
//! semantic style/layout types ([`Style`], [`Palette`], [`Layout`],
//! [`WidthPolicy`]) that [`render`] turns into terminal or JSON output.

/// Comfy-table based terminal rendering and ANSI styling engine.
pub mod render;

/// The one output frame (`header → rule → body → rule`) every `fml` command
/// shares — see [`frame::Frame`].
pub mod frame;

pub use frame::Frame;
pub use render::{
  Table, detect_terminal_width, max_line_display_width, render, render_json,
  separator_for_content, separator_line, strip_ansi_escapes,
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
  /// Unstyled plain text.
  #[default]
  Plain,
  /// Dimmed / muted text.
  Dim,
  /// Bold / emphasized text.
  Strong,
  /// File or directory path text.
  Path,
  /// Tool binary name text.
  Tool,
  /// Success status text (green).
  Ok,
  /// Warning status text (yellow).
  Warn,
  /// Error status text (red).
  Error,
  /// Informational status text (blue/cyan).
  Info,
}

/// A styled segment of text.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Span {
  /// Text content string.
  pub text: String,
  /// Semantic rendering style.
  #[serde(default)]
  pub style: Style,
}

impl Span {
  /// Constructs a [`Span`] with given text and style.
  pub fn new(text: impl Into<String>, style: Style) -> Self {
    Self {
      text: text.into(),
      style,
    }
  }

  /// Constructs a plain (unstyled) [`Span`].
  pub fn plain(text: impl Into<String>) -> Self {
    Self::new(text, Style::Plain)
  }

  /// Constructs a styled [`Span`].
  pub fn styled(text: impl Into<String>, style: Style) -> Self {
    Self::new(text, style)
  }

  /// Computes display character width of text.
  #[must_use]
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
  /// Left-align content.
  #[default]
  Left,
  /// Center-align content.
  Center,
  /// Right-align content.
  Right,
}

/// Overflow handling policy when content exceeds column bounds.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
  /// Wrap overflowing lines.
  #[default]
  Wrap,
  /// Truncate content with a suffix string.
  Truncate {
    /// Suffix appended when truncating.
    #[serde(default = "default_truncate_suffix")]
    suffix: String,
  },
  /// Clip overflowing content cleanly.
  Clip,
}

fn default_truncate_suffix() -> String {
  "...".to_string()
}

/// A single cell inside a table row, composed of semantic spans.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Cell {
  /// Spans comprising cell content.
  pub spans: Vec<Span>,
  /// Optional cell alignment override.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub align: Option<Align>,
  /// Optional cell overflow policy override.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub overflow: Option<Overflow>,
}

impl Cell {
  /// Constructs a [`Cell`] from a vector of spans.
  #[must_use]
  pub fn new(spans: Vec<Span>) -> Self {
    Self {
      spans,
      align: None,
      overflow: None,
    }
  }

  /// Constructs a single plain text [`Cell`].
  pub fn text(text: impl Into<String>) -> Self {
    Self {
      spans: vec![Span::plain(text)],
      align: None,
      overflow: None,
    }
  }

  /// Constructs a styled text [`Cell`].
  pub fn styled(text: impl Into<String>, style: Style) -> Self {
    Self {
      spans: vec![Span::styled(text, style)],
      align: None,
      overflow: None,
    }
  }

  /// Computes display width of all spans in cell.
  #[must_use]
  pub fn display_width(&self) -> usize {
    self.spans.iter().map(Span::display_width).sum()
  }

  /// Sets alignment override for this cell.
  #[must_use]
  pub fn align(mut self, align: Align) -> Self {
    self.align = Some(align);
    self
  }

  /// Sets overflow policy override for this cell.
  #[must_use]
  pub fn overflow(mut self, overflow: Overflow) -> Self {
    self.overflow = Some(overflow);
    self
  }

  /// Appends a span to cell.
  pub fn push(&mut self, span: Span) {
    self.spans.push(span);
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
    Cell::new(vec![s])
  }
}

/// Color rendering mode for terminal palettes.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum PaletteMode {
  /// Plain text without ANSI colors.
  None,
  /// 16-color standard ANSI output.
  #[default]
  Ansi16,
  /// 24-bit RGB truecolor output.
  Truecolor,
}

/// Semantic palette that maps `Style` to ANSI escape codes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
  /// Active palette mode.
  pub mode: PaletteMode,
}

impl Palette {
  /// Creates a [`Palette`] for given mode.
  #[must_use]
  pub fn new(mode: PaletteMode) -> Self {
    Self { mode }
  }

  /// Creates a plain uncolored [`Palette`].
  #[must_use]
  pub fn none() -> Self {
    Self {
      mode: PaletteMode::None,
    }
  }

  /// Creates an ANSI 16-color [`Palette`].
  #[must_use]
  pub fn ansi16() -> Self {
    Self {
      mode: PaletteMode::Ansi16,
    }
  }

  /// Creates a 24-bit RGB truecolor [`Palette`].
  #[must_use]
  pub fn truecolor() -> Self {
    Self {
      mode: PaletteMode::Truecolor,
    }
  }

  /// Returns palette mode.
  #[must_use]
  pub fn mode(&self) -> PaletteMode {
    self.mode
  }

  /// Automatically detect the terminal color capability, respecting standard
  /// environment variables (`NO_COLOR`, `FORCE_COLOR`, `CLICOLOR_FORCE`, COLORTERM, TERM).
  #[must_use]
  pub fn detect() -> Self {
    // 1. Respect NO_COLOR if set and non-empty
    if crate::ui::no_color_requested() {
      return Self::none();
    }

    // 2. Forced color overrides
    let force_color = crate::ui::color_forced();

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
  #[must_use]
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
  #[must_use]
  pub fn apply(&self, text: &str, style: Style) -> String {
    if text.is_empty() {
      return String::new();
    }
    let (prefix, suffix) = self.style_sgr(style);
    if prefix.is_empty() && suffix.is_empty() {
      text.to_string()
    } else {
      format!("{prefix}{text}{suffix}")
    }
  }
}

/// Width policy for a table column.
#[derive(
  Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Debug, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum WidthPolicy {
  /// Automatic width determination based on content.
  #[default]
  Auto,
  /// Fixed column width in characters.
  Fixed(u16),
  /// Minimum column width.
  Min(u16),
  /// Maximum column width.
  Max(u16),
  /// Range of acceptable column widths (min, max).
  Range(u16, u16),
  /// Percentage of available table width.
  Pct(u8),
}

/// Semantic kind of a table row.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
  /// Standard data row.
  #[default]
  Data,
  /// Horizontal rule divider row.
  Rule,
  /// Blank spacing row.
  Blank,
  /// Group header row with title string.
  Group(String),
}

/// Column configuration including header, alignment, width policy, and overflow rule.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Column {
  /// Column header cell content.
  pub header: Cell,
  /// Default text alignment.
  #[serde(default)]
  pub align: Align,
  /// Column width policy.
  #[serde(default)]
  pub width: WidthPolicy,
  /// Column overflow policy.
  #[serde(default)]
  pub overflow: Overflow,
}

impl Column {
  /// Creates a new [`Column`] with header cell.
  pub fn new(header: impl Into<Cell>) -> Self {
    Self {
      header: header.into(),
      align: Align::Left,
      width: WidthPolicy::Auto,
      overflow: Overflow::Wrap,
    }
  }

  /// Sets column text alignment.
  #[must_use]
  pub fn align(mut self, align: Align) -> Self {
    self.align = align;
    self
  }

  /// Sets column width policy.
  #[must_use]
  pub fn width(mut self, width: WidthPolicy) -> Self {
    self.width = width;
    self
  }

  /// Sets column overflow policy.
  #[must_use]
  pub fn overflow(mut self, overflow: Overflow) -> Self {
    self.overflow = overflow;
    self
  }
}

/// A row in the table containing cells and rendering metadata.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Row {
  /// Cells in row.
  pub cells: Vec<Cell>,
  /// Optional maximum row height.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub max_height: Option<usize>,
  /// Row semantic kind.
  #[serde(default)]
  pub kind: RowKind,
}

impl Row {
  /// Constructs a data [`Row`] with given cells.
  #[must_use]
  pub fn new(cells: Vec<Cell>) -> Self {
    Self {
      cells,
      max_height: None,
      kind: RowKind::Data,
    }
  }

  /// Constructs a horizontal rule divider [`Row`].
  #[must_use]
  pub fn rule() -> Self {
    Self {
      cells: Vec::new(),
      max_height: None,
      kind: RowKind::Rule,
    }
  }

  /// Constructs a blank spacing [`Row`].
  #[must_use]
  pub fn blank() -> Self {
    Self {
      cells: Vec::new(),
      max_height: None,
      kind: RowKind::Blank,
    }
  }

  /// Constructs a group header [`Row`] with title.
  pub fn group(title: impl Into<String>) -> Self {
    Self {
      cells: Vec::new(),
      max_height: None,
      kind: RowKind::Group(title.into()),
    }
  }

  /// Sets maximum height constraint for row.
  #[must_use]
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
  /// Compact table spacing.
  #[default]
  Compact,
  /// Comfortable spacious table layout.
  Comfortable,
}

/// Geometry and layout settings for table rendering.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Layout {
  /// Maximum overall table width in characters.
  #[serde(default = "default_max_width")]
  pub max_width: u16,
  /// Whether to constrain table width to terminal window dimensions.
  #[serde(default = "default_clamp_to_terminal")]
  pub clamp_to_terminal: bool,
  /// Cell padding (left, right) in spaces.
  #[serde(default = "default_padding")]
  pub padding: (u16, u16),
  /// Table layout density setting.
  #[serde(default)]
  pub density: Density,
  /// Left indentation offset spaces for table.
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
  /// Creates compact layout settings.
  #[must_use]
  pub fn compact() -> Self {
    Self::default()
  }

  /// Creates comfortable layout settings.
  #[must_use]
  pub fn comfortable() -> Self {
    Self {
      density: Density::Comfortable,
      padding: (1, 1),
      ..Default::default()
    }
  }

  /// Sets maximum table width constraint.
  #[must_use]
  pub fn max_width(mut self, width: u16) -> Self {
    self.max_width = width;
    self
  }

  /// Sets left indentation offset.
  #[must_use]
  pub fn indent(mut self, indent: u16) -> Self {
    self.indent = indent;
    self
  }

  /// Sets cell padding (left, right).
  #[must_use]
  pub fn padding(mut self, left: u16, right: u16) -> Self {
    self.padding = (left, right);
    self
  }

  /// Sets terminal width clamping flag.
  #[must_use]
  pub fn clamp_to_terminal(mut self, clamp: bool) -> Self {
    self.clamp_to_terminal = clamp;
    self
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
#[path = "tests.rs"]
mod tests;
