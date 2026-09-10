//! [`Frame`]: the single presentation frame every `fml` command shares for
//! printed output. A section renders as `header → rule → body → rule`, and one
//! `Frame`, built once per command from its primary table, guarantees every
//! rule that command prints is the same width.

use super::render::detect_terminal_width;
use super::wrap;
use super::{
  Palette, Style, max_line_display_width, separator_line, strip_ansi_escapes,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
  /// Used by `fml doctor --install`'s live progress output, which streams before any
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

/// The continuation ("hanging") indent for a wrapped prose line: its leading
/// whitespace, plus any list bullet (`•`/`-`/`*`) and `[TAG]` label prefix, so
/// a wrapped `  • [WARN]  message…` continues aligned under `message`, not back
/// at column 0. Computed on the ANSI-stripped text; capped at `max` columns.
fn hang_indent(line: &str, max: usize) -> String {
  let plain = strip_ansi_escapes(line);
  let bytes = plain.as_bytes();
  let mut i = 0;
  while i < bytes.len() && bytes[i] == b' ' {
    i += 1;
  }
  for bullet in ["\u{2022} ", "- ", "* "] {
    if plain[i..].starts_with(bullet) {
      i += bullet.len();
      break;
    }
  }
  if let Some(after_bracket) = plain[i..].strip_prefix('[')
    && let Some(close) = after_bracket.find(']')
  {
    let tag = &after_bracket[..close];
    let tagish = !tag.is_empty()
      && tag.len() <= 12
      && tag
        .chars()
        .all(|c| c.is_ascii_uppercase() || c == ' ' || c == '/');
    if tagish {
      let mut j = i + 1 + close + 1;
      while plain[j..].starts_with(' ') {
        j += 1;
      }
      i = j;
    }
  }
  let cols: usize = plain[..i]
    .chars()
    .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
    .sum();
  " ".repeat(cols.min(max))
}

/// Wrap one already-styled line to `width`, preserving ANSI escapes. The first
/// line keeps the source line's own leading whitespace; every continuation
/// line is hung under the text after any bullet / `[TAG]` prefix (see
/// [`hang_indent`]). Returns the line untouched when it already fits.
fn wrap_prose_line(line: &str, width: usize) -> String {
  let width = width.max(8);
  if max_line_display_width(line) <= width {
    return line.to_string();
  }
  let lead_len = line.chars().take_while(|c| *c == ' ').count();
  let lead = " ".repeat(lead_len.min(width / 2));
  let hang = hang_indent(line, width / 2);
  let rest: String = line.chars().skip(lead_len).collect();

  let mut lines: Vec<String> = vec![lead.clone()];
  let mut cur_w = lines[0].chars().count();
  // Every hard-split piece is capped to fit alongside whichever indent (the
  // first line's `lead`, or a continuation's `hang`) it lands after — the
  // larger of the two, so no matter which line a piece falls on, indent +
  // piece never exceeds `width`.
  let hard_split_budget = width
    .saturating_sub(hang.chars().count().max(lead.chars().count()))
    .max(1);
  for unit in wrap::units(&rest) {
    let at_line_start = lines.last().is_some_and(|l| l.trim().is_empty());
    if unit.is_space {
      if !at_line_start && cur_w < width {
        lines.last_mut().unwrap().push(' ');
        cur_w += 1;
      }
      continue;
    }
    if unit.width > width {
      // An unbreakable token (a long path or URL) wider than the whole
      // frame: hard-split it rather than let it overflow, the same
      // last-resort table cells already take (see `wrap::hard_split`).
      let mut fresh = at_line_start;
      for piece in wrap::hard_split(&unit.text, hard_split_budget) {
        if !fresh {
          lines.push(hang.clone());
          cur_w = hang.chars().count();
        }
        lines.last_mut().unwrap().push_str(&piece);
        cur_w += piece.as_str().width();
        fresh = false;
      }
      continue;
    }
    if !at_line_start && cur_w + unit.width > width {
      lines.push(hang.clone());
      cur_w = hang.chars().count();
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

  #[test]
  fn wrap_body_hangs_continuations_under_bullet_and_tag() {
    let frame = Frame { width: 40 };
    let line = "  \u{2022} [WARN]  alpha beta gamma delta epsilon zeta eta \
                theta iota";
    let wrapped = frame.wrap_body(line);
    let out: Vec<&str> = wrapped.lines().collect();
    assert!(out.len() >= 2, "should have wrapped: {wrapped:?}");
    assert!(out[0].starts_with("  \u{2022} [WARN]  "));
    // Continuations hang under the text after the bullet + tag (12 columns:
    // "  " + "• " + "[WARN]  "), not back at column 0 or column 2.
    for cont in &out[1..] {
      let lead = cont.len() - cont.trim_start().len();
      assert_eq!(lead, 12, "continuation not hang-indented: {cont:?}");
    }
    for l in &out {
      assert!(max_line_display_width(l) <= 40, "over width: {l:?}");
    }
  }

  #[test]
  fn wrap_body_leaves_short_lines_untouched() {
    let frame = Frame { width: 40 };
    let line = "  \u{2022} short enough";
    assert_eq!(frame.wrap_body(line), line);
  }

  /// #269: `render::wrap_spans` (table cells) and `frame::wrap_prose_line`
  /// (prose) now share one tokenizer and break-character policy. On text with
  /// no leading indent / bullet / `[TAG]` (so `wrap_prose_line`'s `lead` and
  /// `hang` are both empty, making its loop structurally identical to
  /// `wrap_spans`'s), the two must lay tokens onto lines identically — for
  /// ordinary prose, a path full of `/`, a comma-separated list, text
  /// carrying ANSI escapes, and a 200-character unbreakable token that
  /// neither can break on a separator and both must hard-split the same way.
  /// A future edit that reintroduces two divergent tokenizers/hard-splitters
  /// fails this test.
  #[test]
  fn table_and_prose_wrap_agree_on_shared_corpus() {
    use super::super::Span;
    use super::super::render::wrap_spans;

    let unbreakable = "u".repeat(200);
    let corpus = format!(
      "The quick brown fox jumps over the lazy dog then trots down \
       usr/local/bin/formatter past a/b/c;d/e and hits {unbreakable} head \
       on with \u{1b}[1mstyled\u{1b}[0m flair, twice, for good measure."
    );

    for width in [20usize, 40, 80] {
      let table_lines: Vec<String> =
        wrap_spans(&[Span::plain(corpus.as_str())], width)
          .iter()
          .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
          .collect();

      let prose_lines: Vec<String> = wrap_prose_line(&corpus, width)
        .lines()
        .map(str::to_string)
        .collect();

      assert_eq!(
        table_lines, prose_lines,
        "table and prose wrap diverged at width {width}:\n\
         table: {table_lines:?}\nprose: {prose_lines:?}"
      );

      for line in table_lines.iter().chain(prose_lines.iter()) {
        assert!(
          max_line_display_width(line) <= width,
          "line exceeds width {width}: {line:?}"
        );
      }
    }
  }
}
