//! One shared rendering for filesystem paths in user-facing output: relative
//! to the run root when the path lies under it, absolute only when it genuinely
//! does not. Used by both the table cells and the diagnostics block so every
//! path `fml` prints reads the same way.

use crate::ui::table::strip_ansi_escapes;
use std::path::{Path, PathBuf};

/// Whether `s` already names an absolute location, judged **host-independently**
/// — a Windows-style root (`C:\…`, `C:/…`, `\\server\…`) counts even when this
/// code runs on Linux, and a POSIX `/…` counts on Windows. Diagnostic and diff
/// text carries whatever separators the machine that produced it used, which is
/// not necessarily the machine now running `fml`.
#[must_use]
fn looks_absolute(s: &str) -> bool {
  let b = s.as_bytes();
  s.starts_with('/')
    || s.starts_with('\\')
    || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// Make `p` absolute against the current directory when it is not already
/// rooted. No filesystem access and no lexical `.`/`..` collapsing — run roots
/// and tool-reported paths are effectively always already clean, and rebuilding
/// them component-by-component drops the Windows drive prefix.
fn absolutize(p: &Path) -> PathBuf {
  // `has_root()` (not `is_absolute()`) so a POSIX-style `/a/b` counts as rooted
  // on Windows too — diagnostic text and roots can arrive in either flavor.
  if p.has_root() {
    p.to_path_buf()
  } else {
    std::path::absolute(p)
      .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(p))
  }
}

/// The textual prefixes that mean "under `root`": the root string, trimmed of a
/// trailing separator, in every plausible spelling — as given, all-forward-
/// slash, and all-backslash — each with a trailing `/` and a trailing `\`.
/// Longest first so the most specific spelling wins.
///
/// Deliberately does **not** route through [`absolutize`] / [`Path`] joins:
/// on Linux a `C:\…` or `C:/…` root has no `Path` root, so joining it onto the
/// cwd would yield `/cwd/C:/…` and match nothing. `looks_absolute` classifies
/// it host-independently instead; only a genuinely relative root is anchored to
/// the cwd (as a raw string, keeping its separators).
fn root_prefixes(root: &Path) -> Vec<String> {
  let raw = root.to_string_lossy();
  let base: String = if looks_absolute(&raw) {
    raw.into_owned()
  } else {
    std::path::absolute(root)
      .unwrap_or_else(|_| {
        std::env::current_dir().unwrap_or_default().join(root)
      })
      .to_string_lossy()
      .into_owned()
  };
  let trimmed = base.trim_end_matches(['/', '\\']);
  let clean_trimmed = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
  let mut spellings = vec![
    clean_trimmed.to_string(),
    clean_trimmed.replace('\\', "/"),
    clean_trimmed.replace('/', "\\"),
  ];
  let verbatim_bs = format!(r"\\?\{clean_trimmed}");
  let verbatim_fwd = format!("//?/{clean_trimmed}");
  spellings.push(verbatim_bs.clone());
  spellings.push(verbatim_bs.replace('/', "\\"));
  spellings.push(verbatim_fwd.clone());
  spellings.push(verbatim_fwd.replace('\\', "/"));

  let mut variants: Vec<String> = Vec::new();
  for s in &spellings {
    variants.push(format!("{s}/"));
    variants.push(format!("{s}\\"));
  }
  // A bare separator prefix would strip a leading `/`/`\` off every allowlisted
  // line — never emit one (happens only for a `/` or empty root).
  variants.retain(|v| v.len() > 1);
  variants.sort_by_key(|v| std::cmp::Reverse(v.len()));
  variants.dedup();
  variants
}

/// Render `path` for display: relative to `root` (forward slashes) when it is
/// under `root`, otherwise the path unchanged.
#[must_use]
pub fn display_path(root: &Path, path: &Path) -> String {
  let abs_root = absolutize(root);
  let abs_path = absolutize(path);
  match abs_path.strip_prefix(&abs_root) {
    Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
    Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
    Err(_) => path.to_string_lossy().into_owned(),
  }
}

/// Whether `c` can be part of a path token — so a `root` prefix match
/// *preceded* by one of these is a false positive: a longer path that merely
/// contains the root string (`/mnt/backup/home/u/proj/…` for root
/// `/home/u/proj`), not a path that starts at the root.
#[must_use]
fn is_path_char(c: char) -> bool {
  c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '~' | '/' | '\\' | ':')
}

/// The byte position immediately after a complete, terminated ANSI escape
/// sequence ending exactly at `end`, or `None` if no such sequence abuts
/// `end`. Recognizes the three shapes [`strip_ansi_escapes`] does — CSI
/// (`\x1b[` params/intermediates `final`, final in `0x40..=0x7e`), OSC
/// (`\x1b]` payload terminated by BEL `\x07` or ST `\x1b` + `\`), and a bare
/// two-byte `ESC final` (`final` in `0x30..=0x7e`) — but, unlike
/// [`strip_ansi_escapes`], never looks *through* an OSC payload: it either
/// recognizes a complete sequence terminating right at `end`, or treats the
/// byte at `end - 1` as ordinary text. All recognized bytes (`ESC`, `[`,
/// `]`, `\`, `BEL`, and the CSI/simple-ESC final-byte ranges) are ASCII, so
/// byte-level comparison is safe: none of them can occur as a UTF-8
/// continuation byte, regardless of what non-ASCII text sits inside a
/// payload.
#[must_use]
fn escape_seq_start(s: &str, end: usize) -> Option<usize> {
  let b = s.as_bytes();
  csi_start(b, end)
    .or_else(|| osc_start(b, end))
    .or_else(|| simple_esc_start(b, end))
}

/// `\x1b[` params/intermediates (`0x20..=0x3f`) `final` (`0x40..=0x7e`),
/// ending exactly at `end`.
fn csi_start(b: &[u8], end: usize) -> Option<usize> {
  if end == 0 || !(0x40..=0x7e).contains(&b[end - 1]) {
    return None;
  }
  let mut k = end - 1;
  while k > 0 && (0x20..=0x3f).contains(&b[k - 1]) {
    k -= 1;
  }
  (k >= 2 && b[k - 1] == b'[' && b[k - 2] == 0x1b).then_some(k - 2)
}

/// `\x1b]` payload terminated by BEL or `\x1b` + `\` (ST), ending exactly at
/// `end`. Bails out (returns `None`) rather than guess if the payload
/// itself contains a bare `ESC` or `BEL` before the introducer is found —
/// real OSC payloads (e.g. an OSC-8 `file://` URI) don't, so this covers
/// the sequences that matter without risking a false match.
fn osc_start(b: &[u8], end: usize) -> Option<usize> {
  const BACKSLASH: u8 = 0x5c;
  if end >= 1 && b[end - 1] == 0x07 {
    return find_osc_intro(b, end - 1);
  }
  if end >= 2 && b[end - 1] == BACKSLASH && b[end - 2] == 0x1b {
    return find_osc_intro(b, end - 2);
  }
  None
}

/// Scan back from `end` (exclusive) for the `\x1b]` that opens the OSC
/// payload ending at `end`.
fn find_osc_intro(b: &[u8], end: usize) -> Option<usize> {
  let mut i = end;
  while i >= 2 {
    if b[i - 2] == 0x1b && b[i - 1] == b']' {
      return Some(i - 2);
    }
    if b[i - 1] == 0x1b || b[i - 1] == 0x07 {
      return None;
    }
    i -= 1;
  }
  None
}

/// A bare `ESC final` (`final` in `0x30..=0x7e`), with optional
/// intermediate bytes (`0x20..=0x2f`) in between, ending exactly at `end`.
fn simple_esc_start(b: &[u8], end: usize) -> Option<usize> {
  if end == 0 || !(0x30..=0x7e).contains(&b[end - 1]) {
    return None;
  }
  let mut k = end - 1;
  while k > 0 && (0x20..=0x2f).contains(&b[k - 1]) {
    k -= 1;
  }
  (k >= 1 && b[k - 1] == 0x1b).then_some(k - 1)
}

/// The character immediately preceding `idx` in `s`, skipping back over any
/// complete ANSI escape sequence(s) that end exactly at `idx` (see
/// [`escape_seq_start`]). Unlike stripping ANSI from the whole prefix, this
/// never discards an escape sequence's *payload* as if it weren't there —
/// only a fully recognized, terminated sequence immediately abutting `idx`
/// is skipped, so a plain-text char that merely happens to sit inside e.g.
/// an OSC-8 hyperlink's `file://` URI is still seen and still counts as a
/// real preceding character.
fn char_before_ansi(s: &str, mut idx: usize) -> Option<char> {
  loop {
    if idx == 0 {
      return None;
    }
    match escape_seq_start(s, idx) {
      Some(start) => idx = start,
      None => return s[..idx].chars().next_back(),
    }
  }
}

/// How much of an eligible line [`relativize_line`] may rewrite.
///
/// The two eligibility arms in [`relativize_text`] differ in kind, so they
/// differ here too. A unified-diff file header's *entire* payload is a path
/// by construction, so rewriting every token-boundary occurrence on it is
/// correct. A `<path>:<line>:<col> message` diagnostic's payload is arbitrary
/// text — anything after the leading path may be echoed file *content*, which
/// must not be rewritten (#183).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RewriteScope {
  /// Every occurrence that begins at a real token boundary. Used by the
  /// [`RELATIVIZE_LINE_PREFIXES`] arm only.
  WholeLine,
  /// Only the line's leading token — the occurrence with nothing but ANSI
  /// escape sequences before it. Used by the leading-root-path arm.
  LeadingTokenOnly,
}

/// Whether the prefix occurrence starting at byte `idx` of `line` may be
/// rewritten under `scope`.
///
/// The single place both arms' boundary decisions are made, so raw-vs-stripped
/// reasoning stays in one tested helper rather than being re-derived per call
/// site. `idx` is always an offset into the *raw*, still-colored `line`, and
/// [`char_before_ansi`] is the only thing that interprets what sits before it;
/// nothing here ever indexes ANSI-stripped text.
///
/// `None` from [`char_before_ansi`] means "nothing at all precedes `idx` but
/// complete escape sequences" — i.e. this occurrence *is* the line's leading
/// token — which satisfies both scopes. Any real preceding character satisfies
/// only [`RewriteScope::WholeLine`], and then only when it is not itself a
/// path char (a longer path that merely contains the root string, see
/// [`is_path_char`]).
#[must_use]
fn occurrence_is_rewritable(
  line: &str,
  idx: usize,
  scope: RewriteScope,
) -> bool {
  match char_before_ansi(line, idx) {
    None => true,
    Some(c) => scope == RewriteScope::WholeLine && !is_path_char(c),
  }
}

/// Strip a leading `root/` from the occurrences in `line` that `scope` allows
/// and that begin at a real token boundary (see [`occurrence_is_rewritable`]).
///
/// `line` is the *raw*, still-colored text — splicing it, not a stripped
/// copy, is what keeps styling intact (see [`relativize_text`]) — so the
/// boundary check is done with [`char_before_ansi`] rather than by looking
/// at the raw byte before `idx` directly. A colored path's first character
/// is typically preceded by the `m` that ends an SGR sequence
/// (`\x1b[31m`), which is not itself a real path char but *would* be read
/// as one (`m` is alphanumeric) if the escape sequence weren't recognized
/// and skipped — that was #182: an eligible colored line silently left
/// unrewritten. [`char_before_ansi`] deliberately does *not* strip ANSI
/// from the whole prefix the way [`strip_ansi_escapes`] does, because that
/// would also discard OSC *payload* text (e.g. the `file://` URI inside an
/// OSC-8 hyperlink) as if nothing preceded the match — corrupting the
/// escape sequence itself rather than merely leaving the line unrewritten.
/// It only ever skips a complete, terminated escape sequence that ends
/// exactly at the position being checked.
///
/// Scoped to escapes immediately *before* the matched prefix — an escape
/// sequence landing *inside* the root path itself (splitting `root` across
/// two SGR spans) is not handled: `find` requires the prefix to be
/// contiguous in the raw text, so a split prefix never matches at all and
/// the line silently ships absolute. Tracked as #203, not fixed here.
fn relativize_line(
  line: &str,
  prefixes: &[String],
  scope: RewriteScope,
) -> String {
  let mut out = line.to_string();
  for prefix in prefixes {
    let mut from = 0;
    while let Some(rel) = out[from..].find(prefix.as_str()) {
      let idx = from + rel;
      if occurrence_is_rewritable(&out, idx, scope) {
        out.replace_range(idx..idx + prefix.len(), "");
        if scope == RewriteScope::LeadingTokenOnly {
          // The leading token is rewritten at most once per line, and the
          // prefixes are ordered longest-first, so the most specific
          // spelling has already won — nothing left to look at.
          return out;
        }
        from = idx;
      } else {
        // Under `LeadingTokenOnly` the scan deliberately keeps going rather
        // than stopping at the first rejected occurrence: an earlier match
        // can sit *inside* a complete escape sequence's payload (an OSC-8
        // hyperlink URI wrapping the very path being reported), and
        // `char_before_ansi` skips that whole sequence, so the line's
        // leading token can legitimately come after it.
        from = idx + prefix.len();
      }
    }
  }
  out
}

/// Line prefixes — matched on the ANSI-stripped text, at column 0 — whose
/// entire payload is filesystem paths and is therefore always safe to
/// relativize: unified-diff file headers.
///
/// `"Finding: "` (markdownlint-cli2's echo of its absolute input-file list)
/// used to live here too, but markdown's own noise filter
/// (`filter_markdownlint_noise`) now drops that line before diagnostics ever
/// reach this helper, and markdownlint was the only producer of it — so an
/// entry for it here would never match. Don't re-add it speculatively; if a
/// future surface starts a diagnostic line with `"Finding: "` and needs it
/// relativized, add it back then, with a test that exercises it.
const RELATIVIZE_LINE_PREFIXES: [&str; 4] =
  ["--- ", "+++ ", "diff --git ", "Diff in "];

/// Rewrite absolute paths that lie under `root` to their `root`-relative form,
/// leaving every other path and all other text untouched.
///
/// Deliberately **not** a general search-and-replace, and **not** a diff-state
/// machine (color codes and the leading-space context marker both defeat that).
/// A line is rewritten only when, after ANSI stripping, it either:
/// - begins with one of [`RELATIVIZE_LINE_PREFIXES`] — a line whose whole
///   payload is paths (unified-diff file headers). *Every* token-boundary
///   occurrence on such a line is rewritten
///   ([`RewriteScope::WholeLine`]), which is correct by construction: a
///   unified-diff header carries nothing but paths, or
/// - begins with `root` itself (any [`root_prefixes`] spelling) — a line
///   whose *leading token* is an absolute path under `root`, the shape
///   compiler- and linter-style tools use for `<path>:<line>:<col> message`
///   diagnostics. Observed live from yamllint, clang-format, clang-tidy and
///   `gofmt -l` when invoked with absolute file arguments; markdownlint-cli2
///   is *not* a live case, since it emits cwd-relative paths and fml always
///   runs it with `current_dir(root)`. Here only that leading token is
///   rewritten ([`RewriteScope::LeadingTokenOnly`]) — see the decision note
///   below.
///
/// # Decision (#183): the leading-path arm rewrites the leading token only
///
/// This arm used to rewrite every token-boundary occurrence on the line, the
/// same as the diff-header arm. That was incidental — both arms shared one
/// code path — not an intended guarantee, and it made the module's promise
/// about file *content* true only for *ineligible* lines. A
/// `<path>:<line>:<col> message` line's payload after the leading path is
/// arbitrary text, and a linter may echo the offending source on it:
///
/// ```text
/// in : /root/README.md:3:1 MD044 [Context: "see /root/secret/notes.txt"]
/// out: README.md:3:1 MD044 [Context: "see secret/notes.txt"]      (wrong)
/// ```
///
/// The quoted path is file content, not a diagnostic location; rewriting it
/// changes what the file is reported to say, and it fails quietly — the line
/// still looks like a plausible diagnostic. So the leading-path arm is now
/// restricted to the token that made the line eligible in the first place,
/// and all-occurrence rewriting is left to the arms whose payload genuinely
/// is paths.
///
/// Every ineligible line (unified-diff context and `+`/`-` hunk bodies, `@@`
/// markers, prose) is passed through byte-for-byte, and on an eligible
/// `<path>:<line>:<col>` line everything after the leading token is passed
/// through too — so file *content* that embeds the run-root path is never
/// corrupted, on any line. On a rewritten line the path text is spliced out
/// of the original, so any ANSI styling *around* the matched path is
/// preserved — including a hyperlink escape (OSC 8) wrapped around it, see
/// [`relativize_line`]. Within a rewritten diff header the strip is still
/// token-anchored (see [`occurrence_is_rewritable`]) so a sibling dir or a
/// longer superpath is left alone.
///
/// Escapes *around* a matched path — before, or wrapping it — are handled;
/// an escape sequence landing *inside* the root prefix itself, splitting it
/// across two styled spans, is not: the prefix search requires the root's
/// text to be contiguous in the raw line, so a split prefix simply never
/// matches and that line ships absolute, un-rewritten. Tracked as #203.
#[must_use]
pub fn relativize_text(root: &Path, text: &str) -> String {
  let prefixes = root_prefixes(root);
  text
    .split('\n')
    .map(|line| {
      let plain = strip_ansi_escapes(line);
      // Marker prefixes are checked first: a diff header's payload is paths
      // whatever else the line looks like.
      if RELATIVIZE_LINE_PREFIXES
        .iter()
        .any(|p| plain.starts_with(p))
      {
        relativize_line(line, &prefixes, RewriteScope::WholeLine)
      } else if prefixes.iter().any(|p| plain.starts_with(p.as_str())) {
        relativize_line(line, &prefixes, RewriteScope::LeadingTokenOnly)
      } else {
        line.to_string()
      }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn display_path_relativizes_under_root() {
    let root = Path::new("/home/u/proj");
    assert_eq!(
      display_path(root, Path::new("/home/u/proj/src/main.rs")),
      "src/main.rs"
    );
  }

  #[test]
  fn display_path_keeps_absolute_outside_root() {
    let root = Path::new("/home/u/proj");
    assert_eq!(
      display_path(root, Path::new("/usr/bin/rustfmt")),
      "/usr/bin/rustfmt"
    );
  }

  /// A line whose leading token is the absolute root path — the shape
  /// compiler/linter tools use for `<path>:<line>:<col> message` diagnostics
  /// (yamllint, clang-format, clang-tidy, `gofmt -l`) — is eligible even
  /// without one of the fixed marker prefixes.
  ///
  /// **Changed deliberately by #183** (was
  /// `relativize_text_rewrites_all_occurrences_on_a_leading_path_line`, which
  /// asserted `"README.md docs/a.md and /usr/share/x"`). The old
  /// all-occurrences behavior on this arm was incidental — it fell out of
  /// sharing one code path with the `--- `/`+++ ` arms — and it let echoed
  /// file content on a diagnostic line be rewritten. Only the leading token
  /// is rewritten now; the second occurrence stays absolute. The
  /// `--- `/`+++ ` arms keep all-occurrence rewriting, pinned by
  /// `relativize_text_diff_header_arm_rewrites_every_occurrence` below.
  #[test]
  fn display_path_relativizes_under_relative_root() {
    let cwd = std::env::current_dir().unwrap();
    let file = cwd.join("src").join("lib.rs");
    // With "." as relative root
    assert_eq!(display_path(Path::new("."), &file), "src/lib.rs");
    assert_eq!(
      display_path(Path::new("."), Path::new("src/lib.rs")),
      "src/lib.rs"
    );
    assert_eq!(display_path(Path::new("."), Path::new(".")), ".");

    // With "src" as relative root
    assert_eq!(display_path(Path::new("src"), &file), "lib.rs");
    assert_eq!(
      display_path(Path::new("src"), Path::new("src/lib.rs")),
      "lib.rs"
    );
  }

  #[test]
  fn relativize_text_handles_relative_root() {
    let cwd = std::env::current_dir().unwrap();
    let file = cwd.join("src").join("lib.rs");
    let text = format!("{}:1:1 error", file.display());
    let relativized = relativize_text(Path::new("."), &text);
    assert!(
      relativized == "src/lib.rs:1:1 error"
        || relativized == "src\\lib.rs:1:1 error",
      "expected relative path in {relativized}"
    );

    let diff_text = format!("--- {}", file.display());
    let diff_relativized = relativize_text(Path::new("."), &diff_text);
    assert!(
      diff_relativized == "--- src/lib.rs"
        || diff_relativized == "--- src\\lib.rs",
      "expected relative path in {diff_relativized}"
    );
  }

  #[test]
  fn relativize_text_rewrites_only_the_leading_token_on_a_leading_path_line() {
    let root = Path::new("/home/u/proj");
    let text = "/home/u/proj/README.md /home/u/proj/docs/a.md \
                and /usr/share/x";
    assert_eq!(
      relativize_text(root, text),
      "README.md /home/u/proj/docs/a.md and /usr/share/x"
    );
  }

  /// Regression for #182: a colored leading-path diagnostic line used to be
  /// classified eligible (on the ANSI-stripped text) but then spliced
  /// against the raw, still-colored text, where the `m` terminating the SGR
  /// sequence right before the path reads as an `is_path_char` and blocks
  /// the boundary — so the line was silently left unrewritten, defeating the
  /// module's promise that ANSI styling on a rewritten line is preserved
  /// (there'd have been no rewrite at all). Uses the exact shape from the
  /// issue.
  #[test]
  fn relativize_text_rewrites_a_colored_leading_path_line() {
    let root = Path::new("/home/u/project");
    let text = "\x1b[31m/home/u/project/x.rs:1:1 error\x1b[0m";
    assert_eq!(relativize_text(root, text), "\x1b[31mx.rs:1:1 error\x1b[0m");
  }

  /// Same shape as above but with the color applied only to the path token
  /// itself (SGR reset right after it), and a second, later occurrence of
  /// the root path further into the line that should be rewritten too.
  ///
  /// **Changed deliberately by #183**, together with its uncolored
  /// counterpart above (was
  /// `relativize_text_rewrites_all_occurrences_on_a_colored_leading_path_line`,
  /// asserting `"\x1b[31mx.rs\x1b[0m:1:1 error in y.rs"`). PR #191 added this
  /// as the colored counterpart of the same all-occurrences pin, not as a
  /// second independent guarantee, and its doc comment said so — so both
  /// moved to leading-token-only in one change.
  ///
  /// It still pins #182 in the direction that matters: the leading token is
  /// preceded by the `m` of an SGR sequence, so it is only rewritten because
  /// [`char_before_ansi`] recognizes and skips that complete escape.
  #[test]
  fn relativize_text_rewrites_only_the_colored_leading_token() {
    let root = Path::new("/home/u/project");
    let text = "\x1b[31m/home/u/project/x.rs\x1b[0m:1:1 error in \
                /home/u/project/y.rs";
    assert_eq!(
      relativize_text(root, text),
      "\x1b[31mx.rs\x1b[0m:1:1 error in /home/u/project/y.rs"
    );
  }

  /// The pre-existing `--- ` / `+++ ` diff-header arms stay unaffected by the
  /// ANSI-aware boundary check: a colored header still rewrites correctly,
  /// and the fixed boundary logic doesn't change behavior for the
  /// uncolored/plain cases already covered elsewhere in this module.
  #[test]
  fn relativize_text_rewrites_colored_diff_headers() {
    let root = Path::new("/home/u/proj");
    let text = "\x1b[1m--- /home/u/proj/src/main.rs\x1b[0m";
    assert_eq!(relativize_text(root, text), "\x1b[1m--- src/main.rs\x1b[0m");
  }

  /// Regression for O1 raised in QA review of PR #191: the first fix
  /// attempt at #182 stripped ANSI from the whole prefix before `idx` to
  /// judge the boundary, which discards OSC *payload* text (not just
  /// styling bytes) the same way it discards SGR codes — so a match
  /// landing inside an OSC-8 hyperlink's `file://` payload was wrongly
  /// judged to have "nothing real" before it and got spliced, corrupting
  /// the link target. `char_before_ansi` fixes this by only ever skipping
  /// a complete, terminated escape sequence abutting the checked position,
  /// never looking through a payload — so the literal `/` in `file://`
  /// still counts as an ordinary, boundary-rejecting path char.
  ///
  /// Since #183 this line is rewritten under
  /// [`RewriteScope::LeadingTokenOnly`], which would spare the payload on its
  /// own — so the O1 protection is *also* pinned where it stays load-bearing,
  /// on the whole-line diff-header arm, by
  /// `relativize_text_does_not_mangle_an_osc8_payload_on_a_diff_header`.
  #[test]
  fn relativize_text_does_not_mangle_an_osc8_hyperlink_payload() {
    let root = Path::new("/home/u/proj");
    // Already eligible via the leading `x.rs` token; the second occurrence
    // sits inside an OSC-8 hyperlink's URI payload and must survive intact.
    let text = "/home/u/proj/x.rs \x1b]8;;file:///home/u/proj/y.rs\x1b\\y.rs\x1b]8;;\x1b\\";
    assert_eq!(
      relativize_text(root, text),
      "x.rs \x1b]8;;file:///home/u/proj/y.rs\x1b\\y.rs\x1b]8;;\x1b\\"
    );
  }

  /// The exact #182-shaped repro, but wrapped in an OSC-8 hyperlink instead
  /// of an SGR color: the leading path itself must still be rewritten (the
  /// escape *before* it is recognized and skipped)...
  ///
  /// Also pins a subtlety of #183's leading-token-only scan: the first *raw*
  /// occurrence of the root prefix on this line is inside the hyperlink's
  /// URI payload, so the scan must keep looking past it instead of
  /// concluding the leading token was already passed. An earlier draft
  /// stopped at the first rejected occurrence and left this line unrewritten.
  #[test]
  fn relativize_text_rewrites_a_hyperlinked_leading_path_line() {
    let root = Path::new("/home/u/proj");
    let text = "\x1b]8;;file:///home/u/proj/x.rs\x1b\\/home/u/proj/x.rs:1:1 error\x1b]8;;\x1b\\";
    assert_eq!(
      relativize_text(root, text),
      "\x1b]8;;file:///home/u/proj/x.rs\x1b\\x.rs:1:1 error\x1b]8;;\x1b\\"
    );
  }

  /// The #183 repro: a diagnostic line whose message echoes source content
  /// containing a second path under the run root. The leading token is a
  /// diagnostic *location* and is rewritten; the quoted text is file
  /// *content* and must survive byte-for-byte, or `fml` misreports what the
  /// user's file says.
  #[test]
  fn relativize_text_does_not_rewrite_echoed_content_on_a_diagnostic_line() {
    let root = Path::new("/home/u/project");
    let text = "/home/u/project/README.md:3:1 error MD044 \
                [Context: \"see /home/u/project/secret/notes.txt\"]";
    assert_eq!(
      relativize_text(root, text),
      "README.md:3:1 error MD044 \
       [Context: \"see /home/u/project/secret/notes.txt\"]"
    );
  }

  /// The same shape colored, which is the case PR #191 (#182) newly made
  /// reachable: before that fix this line no-op'd entirely, so the echoed
  /// content was safe by accident. Now the line really is rewritten, and the
  /// quoted content is protected on purpose instead.
  #[test]
  fn relativize_text_does_not_rewrite_echoed_content_on_a_colored_line() {
    let root = Path::new("/home/u/proj");
    let text = "\x1b[31m/home/u/proj/README.md\x1b[0m:3:1 MD044 \
                [Context: \"see /home/u/proj/secret/notes.txt\"]";
    assert_eq!(
      relativize_text(root, text),
      "\x1b[31mREADME.md\x1b[0m:3:1 MD044 \
       [Context: \"see /home/u/proj/secret/notes.txt\"]"
    );
  }

  /// The counterpart of #183's restriction: the `--- `/`+++ `/`diff --git `
  /// arm keeps rewriting *every* token-boundary occurrence, because a
  /// unified-diff header's whole payload is paths by construction. `diff
  /// --git` carries two of them on one line, which is exactly why that arm
  /// cannot be narrowed to the leading token.
  #[test]
  fn relativize_text_diff_header_arm_rewrites_every_occurrence() {
    let root = Path::new("/home/u/proj");
    let text = "diff --git /home/u/proj/a/x.rs /home/u/proj/b/x.rs";
    assert_eq!(relativize_text(root, text), "diff --git a/x.rs b/x.rs");
  }

  /// All-occurrence rewriting on the diff-header arm is still
  /// *token-anchored*: a longer path that merely contains the root string is
  /// left alone even though the arm is willing to rewrite non-leading
  /// occurrences. This is what
  /// `relativize_text_does_not_mangle_a_path_that_merely_contains_the_root`
  /// used to pin on the leading-path arm before #183 restricted that arm to
  /// its leading token.
  #[test]
  fn relativize_text_diff_header_arm_still_token_anchors_occurrences() {
    let root = Path::new("/home/u/proj");
    let text = "--- /home/u/proj/a.md /mnt/backup/home/u/proj/b.md \
                /home/u/project/c.md";
    assert_eq!(
      relativize_text(root, text),
      "--- a.md /mnt/backup/home/u/proj/b.md /home/u/project/c.md"
    );
  }

  /// PR #191's O1 regression, re-pinned on the arm where it is still
  /// load-bearing after #183: on a whole-line (diff-header) rewrite, a match
  /// landing inside an OSC-8 hyperlink's `file://` payload must not be
  /// spliced. The `/` of `file://` is a real preceding path char, and
  /// [`char_before_ansi`] must see it rather than looking through the
  /// payload the way `strip_ansi_escapes` would.
  #[test]
  fn relativize_text_does_not_mangle_an_osc8_payload_on_a_diff_header() {
    let root = Path::new("/home/u/proj");
    let text = "--- \x1b]8;;file:///home/u/proj/y.rs\x1b\\y.rs\x1b]8;;\x1b\\";
    assert_eq!(relativize_text(root, text), text);
  }

  /// The shared boundary helper both arms route through, at the level of the
  /// decision itself: "nothing but escapes precedes" satisfies either scope,
  /// a non-path char satisfies only [`RewriteScope::WholeLine`], and a path
  /// char satisfies neither.
  #[test]
  fn occurrence_is_rewritable_distinguishes_the_two_scopes() {
    let cases = [
      // (line, idx, whole_line, leading_only)
      ("/home/u/proj/x.rs", 0, true, true),
      ("\x1b[31m/home/u/proj/x.rs", 5, true, true),
      ("--- /home/u/proj/x.rs", 4, true, false),
      ("\x1b]8;;file:///home/u/proj/x.rs", 12, false, false),
      ("/mnt/backup/home/u/proj/x.rs", 11, false, false),
    ];
    for (line, idx, whole, leading) in cases {
      assert_eq!(
        occurrence_is_rewritable(line, idx, RewriteScope::WholeLine),
        whole,
        "WholeLine at {idx} of {line:?}"
      );
      assert_eq!(
        occurrence_is_rewritable(line, idx, RewriteScope::LeadingTokenOnly),
        leading,
        "LeadingTokenOnly at {idx} of {line:?}"
      );
    }
  }

  #[test]
  fn relativize_text_handles_backslash_separators() {
    let root = Path::new("C:/work/repo");
    let text = "--- C:\\work\\repo\\poly\\data.json (formatted)";
    assert_eq!(
      relativize_text(root, text),
      "--- poly\\data.json (formatted)"
    );
  }

  #[test]
  fn root_prefixes_emits_both_separator_spellings_host_independently() {
    // A Windows-style root: on Linux it has no `Path` root, so this must not
    // route through a cwd join (which was the CI regression).
    let v = root_prefixes(Path::new("C:\\work\\repo"));
    assert!(v.contains(&"C:/work/repo/".to_string()), "{v:?}");
    assert!(v.contains(&"C:\\work\\repo\\".to_string()), "{v:?}");
    // A POSIX root, likewise both spellings.
    let v = root_prefixes(Path::new("/home/u/proj"));
    assert!(v.contains(&"/home/u/proj/".to_string()), "{v:?}");
    assert!(v.contains(&"\\home\\u\\proj\\".to_string()), "{v:?}");
  }

  #[test]
  fn relativize_text_separator_mismatch_between_root_and_text() {
    // backslash root, forward-slash text
    assert_eq!(
      relativize_text(
        Path::new("C:\\work\\repo"),
        "--- C:/work/repo/poly/data.json (formatted)"
      ),
      "--- poly/data.json (formatted)"
    );
    // forward-slash root, backslash text
    assert_eq!(
      relativize_text(
        Path::new("C:/work/repo"),
        "+++ C:\\work\\repo\\poly\\data.json"
      ),
      "+++ poly\\data.json"
    );
  }

  #[test]
  fn looks_absolute_is_host_independent() {
    assert!(looks_absolute("/home/u"));
    assert!(looks_absolute("C:\\x"));
    assert!(looks_absolute("C:/x"));
    assert!(looks_absolute("\\\\server\\share"));
    assert!(!looks_absolute("subdir/file"));
    assert!(!looks_absolute("./rel"));
  }

  #[test]
  fn relativize_text_noop_when_nothing_under_root() {
    let root = Path::new("/home/u/proj");
    let text = "nothing to see /elsewhere/file";
    assert_eq!(relativize_text(root, text), text);
  }

  #[test]
  fn relativize_text_rewrites_leading_path_diagnostic_lines() {
    // The exact shape markdownlint-cli2 (and compiler-style tools generally)
    // emit when it falls back to an absolute path: `<path>:<line>:<col>
    // message`, no fixed marker prefix. This is what let #157 fold
    // markdown's bespoke shim into this shared helper instead of keeping a
    // second, surface-local relativization pass.
    let root = Path::new("/home/u/proj");
    let text = "/home/u/proj/README.md:7:3 error MD019/no-multiple-space-atx \
                Multiple spaces after hash";
    assert_eq!(
      relativize_text(root, text),
      "README.md:7:3 error MD019/no-multiple-space-atx Multiple spaces \
       after hash"
    );
  }

  #[test]
  fn relativize_text_does_not_mangle_a_path_that_merely_contains_the_root() {
    let root = Path::new("/home/u/proj");
    // The line is eligible (it opens with the literal root path), but the
    // other two occurrences are not touched: one has the root string
    // mid-way through an unrelated absolute path (no token boundary before
    // it), the other is a sibling directory sharing a name prefix (the
    // literal substring doesn't even occur).
    //
    // Since #183 this arm wouldn't touch them regardless of the boundary
    // check, so this is no longer the pin for token anchoring — see
    // `relativize_text_diff_header_arm_still_token_anchors_occurrences`,
    // which asserts the same thing on the arm that still rewrites
    // non-leading occurrences. Kept as a plain non-regression assertion.
    let text = "/home/u/proj/a.md refers to /mnt/backup/home/u/proj/b.md \
                and sibling /home/u/project/c.md";
    assert_eq!(
      relativize_text(root, text),
      "a.md refers to /mnt/backup/home/u/proj/b.md and sibling \
       /home/u/project/c.md"
    );
  }

  #[test]
  fn relativize_text_rewrites_diff_headers_but_not_hunk_bodies() {
    let root = Path::new("/home/u/proj");
    // (b) the added/removed lines embed the root path as file *content*; only
    // the `---` / `+++` headers may be rewritten.
    let diff = "--- /home/u/proj/src/main.rs\n\
                +++ /home/u/proj/src/main.rs (formatted)\n\
                @@ -1,2 +1,2 @@\n\
                -let cfg = \"/home/u/proj/config.toml\";\n\
                +let cfg = \"/home/u/proj/config.toml\".to_string();";
    let out = relativize_text(root, diff);
    assert!(out.contains("--- src/main.rs\n"));
    assert!(out.contains("+++ src/main.rs (formatted)\n"));
    assert!(out.contains("-let cfg = \"/home/u/proj/config.toml\";"));
    assert!(
      out.contains("+let cfg = \"/home/u/proj/config.toml\".to_string();")
    );
  }

  #[test]
  fn relativize_text_two_file_plain_diff_relativizes_every_header() {
    let root = Path::new("/home/u/proj");
    let diff = "--- /home/u/proj/a/x.rs\n\
                +++ /home/u/proj/a/x.rs (formatted)\n\
                @@ -1 +1 @@\n-a\n+b\n\
                --- /home/u/proj/b/y.rs\n\
                +++ /home/u/proj/b/y.rs (formatted)\n\
                @@ -1 +1 @@\n-c\n+d";
    let out = relativize_text(root, diff);
    assert!(out.contains("--- a/x.rs\n"));
    assert!(out.contains("+++ a/x.rs (formatted)\n"));
    assert!(out.contains("--- b/y.rs\n"));
    assert!(out.contains("+++ b/y.rs (formatted)\n"));
    assert!(!out.contains("/home/u/proj"));
  }

  /// The real regression: `engine::diff::render_diff` output, with color ON
  /// (the default) and OFF. Only the `---` / `+++` headers may change; every
  /// context / `+` / `-` / `@@` line — including one whose body literally
  /// contains the absolute run-root path — must survive byte-for-byte.
  #[test]
  fn relativize_text_over_real_render_diff_never_touches_hunk_bodies() {
    let root = Path::new("/home/u/proj");
    let old = "use \"/home/u/proj/lib\";\n\
               let p = \"/home/u/proj/x\";\n\
               fn main() {}\n\
               // end\n";
    let new = "use \"/home/u/proj/lib\";\n\
               let p = \"/home/u/proj/x\".into();\n\
               fn main() {}\n\
               // end\n";
    let old_label = "/home/u/proj/src/main.rs";
    let new_label = "/home/u/proj/src/main.rs (formatted)";

    let check = |diff: &str| {
      let out = relativize_text(root, diff);
      assert_eq!(
        diff.lines().count(),
        out.lines().count(),
        "line count changed"
      );
      let mut saw_header = false;
      let mut saw_body_with_root_path = false;
      for (a, b) in diff.lines().zip(out.lines()) {
        let pa = strip_ansi_escapes(a);
        if pa.starts_with("--- ") || pa.starts_with("+++ ") {
          saw_header = true;
          assert_ne!(a, b, "header not relativized: {a:?}");
          assert!(
            !strip_ansi_escapes(b).contains("/home/u/proj/src"),
            "header still absolute: {b:?}"
          );
        } else {
          if pa.contains("/home/u/proj/") {
            saw_body_with_root_path = true;
          }
          assert_eq!(
            a, b,
            "a non-header line was modified — corruption risk: {a:?}"
          );
        }
      }
      assert!(saw_header, "test diff had no file headers");
      assert!(
        saw_body_with_root_path,
        "test diff had no hunk line embedding the root path"
      );
      // Content path preserved; header path gone.
      assert!(strip_ansi_escapes(&out).contains("/home/u/proj/x"));
      assert!(!strip_ansi_escapes(&out).contains("/home/u/proj/src"));
    };

    // `colored`'s override is a process-global — `cargo test` runs this
    // binary's tests in one process, multiple threads. Nothing else in this
    // crate currently touches the override in a test, so there's no
    // concurrent-mutation race today, but a plain `set_override` /
    // `unset_override` pair leaves the global forced-on if an assertion
    // inside `check` panics before the reset runs, which would silently
    // color-force every test that happens to run afterward in this process.
    // `_guard` closes that window: its `Drop` resets the override on the way
    // out whether this test returns normally or unwinds. If a second test
    // ever needs this same override, promote this to a shared
    // `Mutex`-guarded helper so the two can't interleave either — one test
    // doing the mutation doesn't justify that machinery yet.
    struct ColorOverrideGuard;
    impl Drop for ColorOverrideGuard {
      fn drop(&mut self) {
        colored::control::unset_override();
      }
    }
    let _guard = ColorOverrideGuard;

    colored::control::set_override(true);
    check(&crate::engine::render_diff(old, new, old_label, new_label));
    colored::control::set_override(false);
    check(&crate::engine::render_diff(old, new, old_label, new_label));
  }
}
