//! [`Frame`]: the single presentation frame every `fml` command shares for
//! printed output. A section renders as `header → rule → body → rule`, and one
//! `Frame`, built once per command from its primary table, guarantees every
//! rule that command prints is the same width.

use super::render::detect_terminal_width;
use super::{Palette, Style, max_line_display_width, separator_line};
use unicode_width::UnicodeWidthChar;

/// The 80-column output target from issue #122, honored unless the real
/// terminal is genuinely narrower.
pub const TARGET_WIDTH: usize = 80;

/// The narrowest frame we will ever draw, so degenerate inputs (an empty table,
/// a 1-column terminal) still produce something coherent.
const MIN_WIDTH: usize = 8;

/// Shared framing geometry for one command's output.
///
/// Construct once with [`Frame::for_body`] from the command's primary rendered
/// table (or [`Frame::capped`] when there is no table to size against), then
/// wrap every section — the table, each follow-up block — with
/// [`Frame::section`]. Because all of them draw from the same width, the output
/// reads as one tool rather than several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
  width: usize,
}

impl Frame {
  /// The width cap: [`TARGET_WIDTH`], or the real terminal width when narrower.
  #[must_use]
  pub fn cap() -> usize {
    (detect_terminal_width() as usize).min(TARGET_WIDTH)
  }

  /// A frame sized to an already-rendered body: the body's widest line,
  /// clamped to [`Frame::cap`].
  #[must_use]
  pub fn for_body(rendered_body: &str) -> Self {
    let cap = Self::cap();
    let content = max_line_display_width(rendered_body);
    let width = if content == 0 { cap } else { content.min(cap) };
    Self {
      width: width.max(MIN_WIDTH.min(cap)),
    }
  }

  /// A frame with no body to size against — just the [`Frame::cap`] width.
  /// Used by `fml install`'s live progress output, which streams before any
  /// table exists.
  #[must_use]
  pub fn capped() -> Self {
    Self {
      width: Self::cap().max(MIN_WIDTH),
    }
  }

  /// The shared rule width, in columns.
  #[must_use]
  pub fn width(&self) -> usize {
    self.width
  }

  /// The bare (uncolored) rule line.
  #[must_use]
  pub fn rule(&self) -> String {
    separator_line(self.width)
  }

  /// The rule line, dimmed with `palette`.
  #[must_use]
  pub fn dim_rule(&self, palette: &Palette) -> String {
    palette.apply(&self.rule(), Style::Dim)
  }

  /// Wrap free-form prose `text` so no line exceeds [`Frame::width`], breaking
  /// only at spaces and after path separators / punctuation (never mid-token),
  /// and re-applying each source line's own leading indent to its continuation
  /// lines. ANSI styling is copied through untouched. Lines already within the
  /// width are left exactly as-is.
  ///
  /// For notice / diagnostic prose only — not tables, whose columns are
  /// already fitted by [`super::render`].
  #[must_use]
  pub fn wrap_body(&self, text: &str) -> String {
    text
      .split('\n')
      .map(|line| wrap_prose_line(line, self.width))
      .collect::<Vec<_>>()
      .join("\n")
  }

  /// One framed section: `title`, a rule, `body`, a closing rule — the single
  /// framing shape for all `fml` output. `title` and `body` are passed already
  /// styled by the caller (colors differ per section); only the rule is drawn
  /// here. A blank `body` collapses to just `title` + rule. The returned block
  /// has no trailing newline.
  #[must_use]
  pub fn section(&self, title: &str, body: &str, palette: &Palette) -> String {
    let rule = self.dim_rule(palette);
    let body = body.trim_matches('\n');
    if body.is_empty() {
      format!("{title}\n{rule}")
    } else {
      format!("{title}\n{rule}\n{body}\n{rule}")
    }
  }
}

/// Break chars after which a wrap is allowed (a space always allows one).
/// Matches `render`'s policy so tables and prose wrap the same way.
const BREAK_AFTER: [char; 4] = ['/', '\\', ',', ';'];

/// One unit of a prose line: a run of visible text ending at a break point, a
/// single space, or a zero-width ANSI escape sequence.
struct Unit {
  text: String,
  width: usize,
  is_space: bool,
}

/// Split `s` (leading indent already removed) into wrappable [`Unit`]s.
fn units(s: &str) -> Vec<Unit> {
  let mut out = Vec::new();
  let mut cur = String::new();
  let mut cur_w = 0usize;
  let mut chars = s.chars().peekable();
  while let Some(c) = chars.next() {
    if c == '\x1b' {
      // Copy the CSI/SGR sequence verbatim onto the current run; it is
      // zero-width, so it never forces a wrap on its own.
      cur.push(c);
      for e in chars.by_ref() {
        cur.push(e);
        if e.is_ascii_alphabetic() {
          break;
        }
      }
      continue;
    }
    if c == ' ' {
      if !cur.is_empty() {
        out.push(Unit {
          text: std::mem::take(&mut cur),
          width: cur_w,
          is_space: false,
        });
        cur_w = 0;
      }
      out.push(Unit {
        text: " ".to_string(),
        width: 1,
        is_space: true,
      });
      continue;
    }
    cur.push(c);
    cur_w += UnicodeWidthChar::width(c).unwrap_or(0);
    if BREAK_AFTER.contains(&c) {
      out.push(Unit {
        text: std::mem::take(&mut cur),
        width: cur_w,
        is_space: false,
      });
      cur_w = 0;
    }
  }
  if !cur.is_empty() {
    out.push(Unit {
      text: cur,
      width: cur_w,
      is_space: false,
    });
  }
  out
}

/// Wrap one already-styled line to `width`, preserving ANSI escapes and
/// re-indenting continuations to the source line's own leading whitespace.
/// Returns the line untouched when it already fits.
fn wrap_prose_line(line: &str, width: usize) -> String {
  let width = width.max(8);
  if max_line_display_width(line) <= width {
    return line.to_string();
  }
  let indent_len = line.chars().take_while(|c| *c == ' ').count();
  let indent = " ".repeat(indent_len.min(width / 2));
  let rest: String = line.chars().skip(indent_len).collect();

  let mut lines: Vec<String> = vec![indent.clone()];
  let mut cur_w = indent.chars().count();
  for unit in units(&rest) {
    let at_line_start = lines.last().is_some_and(|l| l.trim().is_empty());
    if unit.is_space {
      if !at_line_start && cur_w < width {
        lines.last_mut().unwrap().push(' ');
        cur_w += 1;
      }
      continue;
    }
    if !at_line_start && cur_w + unit.width > width {
      lines.push(indent.clone());
      cur_w = indent.chars().count();
    }
    lines.last_mut().unwrap().push_str(&unit.text);
    cur_w += unit.width;
  }

  lines
    .iter()
    .map(|l| l.trim_end())
    .collect::<Vec<_>>()
    .join("\n")
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn section_is_header_rule_body_rule() {
    let frame = Frame::capped();
    let out = frame.section("TITLE", "line one\nline two", &Palette::none());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "TITLE");
    assert!(lines[1].chars().all(|c| c == '─'));
    assert_eq!(lines[2], "line one");
    assert_eq!(lines[3], "line two");
    assert!(lines[4].chars().all(|c| c == '─'));
    assert_eq!(lines[1], lines[4], "opening and closing rule must match");
  }

  #[test]
  fn empty_body_collapses_to_title_and_one_rule() {
    let frame = Frame::capped();
    let out = frame.section("TITLE", "", &Palette::none());
    assert_eq!(out.lines().count(), 2);
  }

  #[test]
  fn width_never_exceeds_80() {
    assert!(Frame::capped().width() <= TARGET_WIDTH);
    let wide_body = "x".repeat(500);
    assert!(Frame::for_body(&wide_body).width() <= TARGET_WIDTH);
  }

  #[test]
  fn for_body_sizes_to_content_when_under_cap() {
    let body = "a".repeat(40);
    assert_eq!(Frame::for_body(&body).width(), 40);
  }
}
