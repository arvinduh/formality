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
    std::env::current_dir().unwrap_or_default().join(p)
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
    std::env::current_dir()
      .unwrap_or_default()
      .join(root)
      .to_string_lossy()
      .into_owned()
  };
  let trimmed = base.trim_end_matches(['/', '\\']);
  let spellings = [
    trimmed.to_string(),
    trimmed.replace('\\', "/"),
    trimmed.replace('/', "\\"),
  ];

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

/// Strip a leading `root/` from every occurrence in `line` that begins at a
/// real token boundary (start of line, or a non-path char before it).
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
fn relativize_line(line: &str, prefixes: &[String]) -> String {
  let mut out = line.to_string();
  for prefix in prefixes {
    let mut from = 0;
    while let Some(rel) = out[from..].find(prefix.as_str()) {
      let idx = from + rel;
      let boundary_ok =
        idx == 0 || !char_before_ansi(&out, idx).is_some_and(is_path_char);
      if boundary_ok {
        out.replace_range(idx..idx + prefix.len(), "");
        from = idx;
      } else {
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
const RELATIVIZE_LINE_PREFIXES: [&str; 3] = ["--- ", "+++ ", "diff --git "];

/// Rewrite absolute paths that lie under `root` to their `root`-relative form,
/// leaving every other path and all other text untouched.
///
/// Deliberately **not** a general search-and-replace, and **not** a diff-state
/// machine (color codes and the leading-space context marker both defeat that).
/// A line is rewritten only when, after ANSI stripping, it either:
/// - begins with one of [`RELATIVIZE_LINE_PREFIXES`] — a line whose whole
///   payload is paths (unified-diff file headers), or
/// - begins with `root` itself (any [`root_prefixes`] spelling) — a line
///   whose *leading token* is an absolute path under `root`, the shape
///   compiler- and linter-style tools use for `<path>:<line>:<col> message`
///   diagnostics. Observed live from yamllint, clang-format, clang-tidy and
///   `gofmt -l` when invoked with absolute file arguments; markdownlint-cli2
///   is *not* a live case, since it emits cwd-relative paths and fml always
///   runs it with `current_dir(root)`. Column 0 makes this safe: prose
///   or diff-hunk body content that happens to *mention* the root path
///   elsewhere on the line is never touched, only a line that opens with it.
///
/// Every other line (unified-diff context and `+`/`-` hunk bodies, `@@`
/// markers, prose) is passed through byte-for-byte, so file *content* that
/// embeds the run-root path is never corrupted. On a rewritten line the path
/// text is spliced out of the original, so any ANSI styling *around* the
/// matched path is preserved — including a hyperlink escape (OSC 8) wrapped
/// around it, see [`relativize_line`]. Within a rewritten line the strip is
/// still token-anchored (see [`relativize_line`]) so a sibling dir or a
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
      let eligible = RELATIVIZE_LINE_PREFIXES
        .iter()
        .any(|p| plain.starts_with(p))
        || prefixes.iter().any(|p| plain.starts_with(p.as_str()));
      if eligible {
        relativize_line(line, &prefixes)
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

  #[test]
  fn relativize_text_rewrites_all_occurrences_on_a_leading_path_line() {
    // A line whose leading token is the absolute root path — the shape
    // compiler/linter tools use for `<path>:<line>:<col> message` diagnostics
    // (yamllint, clang-format, clang-tidy, `gofmt -l`) — is eligible even
    // without one of the fixed marker prefixes, and every in-bounds
    // occurrence on it is rewritten.
    let root = Path::new("/home/u/proj");
    let text = "/home/u/proj/README.md /home/u/proj/docs/a.md \
                and /usr/share/x";
    assert_eq!(
      relativize_text(root, text),
      "README.md docs/a.md and /usr/share/x"
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
  /// Note for whoever picks up #183: this pins the *all-occurrences*
  /// behavior on an eligible leading-path line — the second, non-leading
  /// occurrence (`y.rs`) is rewritten too, same as
  /// `relativize_text_rewrites_all_occurrences_on_a_leading_path_line`
  /// above pins for the uncolored case. #183 proposes restricting that arm
  /// to the leading token only, which would need this assertion updated
  /// alongside that one — this test isn't asserting a second, independent
  /// guarantee, it's the colored counterpart of the same one.
  #[test]
  fn relativize_text_rewrites_all_occurrences_on_a_colored_leading_path_line() {
    let root = Path::new("/home/u/project");
    let text = "\x1b[31m/home/u/project/x.rs\x1b[0m:1:1 error in \
                /home/u/project/y.rs";
    assert_eq!(
      relativize_text(root, text),
      "\x1b[31mx.rs\x1b[0m:1:1 error in y.rs"
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
  #[test]
  fn relativize_text_rewrites_a_hyperlinked_leading_path_line() {
    let root = Path::new("/home/u/proj");
    let text = "\x1b]8;;file:///home/u/proj/x.rs\x1b\\/home/u/proj/x.rs:1:1 error\x1b]8;;\x1b\\";
    assert_eq!(
      relativize_text(root, text),
      "\x1b]8;;file:///home/u/proj/x.rs\x1b\\x.rs:1:1 error\x1b]8;;\x1b\\"
    );
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
