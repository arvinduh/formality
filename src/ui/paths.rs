//! One shared rendering for filesystem paths in user-facing output: relative
//! to the run root when the path lies under it, absolute only when it genuinely
//! does not. Used by both the table cells and the diagnostics block so every
//! path `fml` prints reads the same way.

use std::path::{Path, PathBuf};

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

/// The set of textual prefixes that mean "under `root`" — the root rendered
/// with both separator styles, each followed by a separator. Longest first, so
/// the most specific match wins.
fn root_prefixes(root: &Path) -> Vec<String> {
  let raw = absolutize(root).to_string_lossy().into_owned();
  let trimmed = raw.trim_end_matches(['/', '\\']);
  let fwd = trimmed.replace('\\', "/");
  let back = trimmed.replace('/', "\\");
  let mut variants = vec![format!("{fwd}/"), format!("{back}\\")];
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

/// Strip a leading `root/` from every occurrence in `line` that begins at a
/// real token boundary (start of line, or a non-path char before it).
fn relativize_line(line: &str, prefixes: &[String]) -> String {
  let mut out = line.to_string();
  for prefix in prefixes {
    let mut from = 0;
    while let Some(rel) = out[from..].find(prefix.as_str()) {
      let idx = from + rel;
      let boundary_ok =
        idx == 0 || !out[..idx].chars().next_back().is_some_and(is_path_char);
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

/// Rewrite absolute paths that lie under `root` to their `root`-relative form,
/// leaving every other path and all other text untouched.
///
/// Conservative on two axes so it can never silently corrupt output:
///
/// * **Token-anchored.** Only a `root`-plus-separator prefix that starts at a
///   real path boundary is stripped, so `/mnt/backup/home/u/proj/a` (root
///   `/home/u/proj`) and a sibling dir `/home/u/project` are left alone.
/// * **Diff-aware.** Unified-diff hunk bodies (lines after an `@@ … @@` marker
///   that start with `+`, `-`, space, or `\`) are file *content* and may
///   legitimately embed an absolute path; those lines are never rewritten. The
///   `---` / `+++` hunk headers, which come before `@@`, still are.
#[must_use]
pub fn relativize_text(root: &Path, text: &str) -> String {
  let prefixes = root_prefixes(root);
  let mut in_hunk = false;
  text
    .split('\n')
    .map(|line| {
      let content = line.trim_start();
      if content.starts_with("@@ ") {
        in_hunk = true;
        return line.to_string();
      }
      if in_hunk {
        if content.is_empty() || content.starts_with(['+', '-', ' ', '\\']) {
          return line.to_string();
        }
        in_hunk = false;
      }
      relativize_line(line, &prefixes)
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
  fn relativize_text_rewrites_all_occurrences() {
    let root = Path::new("/home/u/proj");
    let text = "Finding: /home/u/proj/README.md /home/u/proj/docs/a.md \
                and /usr/share/x";
    assert_eq!(
      relativize_text(root, text),
      "Finding: README.md docs/a.md and /usr/share/x"
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
  fn relativize_text_noop_when_nothing_under_root() {
    let root = Path::new("/home/u/proj");
    let text = "nothing to see /elsewhere/file";
    assert_eq!(relativize_text(root, text), text);
  }

  #[test]
  fn relativize_text_does_not_mangle_a_path_that_merely_contains_the_root() {
    let root = Path::new("/home/u/proj");
    // (a) an unrelated absolute path that has the root string mid-way through,
    // and a sibling directory sharing a name prefix — neither starts at a
    // token boundary, so neither is touched.
    let text = "Finding: /mnt/backup/home/u/proj/a.md /home/u/project/b.md";
    assert_eq!(relativize_text(root, text), text);
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
}
