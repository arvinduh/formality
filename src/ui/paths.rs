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

/// Rewrite every absolute path under `root` that appears anywhere in `text` to
/// its `root`-relative form, leaving paths outside `root` (and all other text)
/// untouched. Heuristic but conservative: only the long, specific
/// `root`-plus-separator prefix is stripped.
#[must_use]
pub fn relativize_text(root: &Path, text: &str) -> String {
  let mut out = text.to_string();
  for prefix in root_prefixes(root) {
    if out.contains(&prefix) {
      out = out.replace(&prefix, "");
    }
  }
  out
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
}
