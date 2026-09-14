//! Shared word-wrap tokenizer for both table cells
//! ([`super::render::wrap_spans`]) and prose blocks
//! ([`super::frame::wrap_prose_line`]). One break-character policy, one
//! tokenizer, so tables and prose wrap the same way — see #269.

use unicode_width::UnicodeWidthChar;

/// Characters after which a soft line break is allowed when wrapping: path
/// separators and list punctuation. A break is also always allowed at a
/// space. Deliberately excludes `.`/`-`/`_`/`:` so `rustfmt.exe`,
/// `v1.9.0-stable`, and `C:` stay glued and remain copy/double-click friendly.
pub(super) const BREAK_AFTER: [char; 4] = ['/', '\\', ',', ';'];

/// The narrowest inner width a caller will shrink a column to as a last
/// resort, once respecting every column's widest-token floor would push the
/// table past its width budget. At this point one token is hard-split.
pub(super) const LAST_RESORT_MIN: usize = 3;

/// One wrap unit: a run of visible text ending at a break point, a single
/// space, or a zero-width ANSI escape sequence carried along on the run that
/// precedes it.
pub(super) struct Unit {
  pub text: String,
  pub width: usize,
  pub is_space: bool,
}

/// Split `s` into wrappable [`Unit`]s: maximal runs that must not be broken
/// across lines. A trailing [`BREAK_AFTER`] char stays with its token; each
/// space becomes its own `" "` unit so a caller can collapse consecutive ones
/// at a wrap point. ANSI CSI/SGR escape sequences are copied verbatim onto the
/// current run and contribute zero width, so they never force a wrap on their
/// own — harmless for callers (e.g. table cell spans) that never see escapes
/// in their input text.
pub(super) fn units(s: &str) -> Vec<Unit> {
  let mut out = Vec::new();
  let mut cur = String::new();
  let mut cur_w = 0usize;
  let mut chars = s.chars().peekable();
  while let Some(c) = chars.next() {
    if c == '\x1b' {
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

/// Display width of the widest single non-space unit in `text` — the minimum
/// inner column width at which `text` can be laid out without splitting a
/// token.
pub(super) fn token_display_width(text: &str) -> usize {
  units(text)
    .iter()
    .filter(|u| !u.is_space)
    .map(|u| u.width)
    .max()
    .unwrap_or(0)
}

/// Last-resort hard split of a single token genuinely wider than `width`.
///
/// Reached from [`super::render::wrap_spans`] whenever a column's resolved
/// inner width is below the token's own display width — which happens for a
/// hard-cap policy (`Max` / `Range` upper / `Pct`) tighter than the token, or
/// for any column that width solving had to shrink past its widest-token
/// floor to keep the whole table within its width budget ([`LAST_RESORT_MIN`]).
/// Also reached from [`super::frame::wrap_prose_line`] for the prose
/// equivalent: an unbreakable path or URL wider than the frame.
pub(super) fn hard_split(text: &str, width: usize) -> Vec<String> {
  let width = width.max(1);
  let mut out = Vec::new();
  let mut cur = String::new();
  let mut w = 0;
  for ch in text.chars() {
    let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
    if w + cw > width && !cur.is_empty() {
      out.push(std::mem::take(&mut cur));
      w = 0;
    }
    cur.push(ch);
    w += cw;
  }
  if !cur.is_empty() {
    out.push(cur);
  }
  out
}
