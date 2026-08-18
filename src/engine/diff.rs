use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use std::fmt::Write as _;

/// Generates a colored unified diff string between `old_content` and `new_content`.
#[must_use]
pub fn render_diff(
  old_content: &str,
  new_content: &str,
  old_label: &str,
  new_label: &str,
) -> String {
  let diff = TextDiff::from_lines(old_content, new_content);
  // Rendered diffs (headers + ANSI styling) are usually comparable in size
  // to the larger of the two inputs; reserving up front avoids repeated
  // reallocation/copying as `out` grows line-by-line below.
  let mut out = String::with_capacity(old_content.len().max(new_content.len()));

  // `write!` formats the already-colored value directly into `out`,
  // whereas the previous `.to_string()` + `push_str` combo allocated a
  // second throwaway `String` per line just to copy it straight back out.
  let _ = write!(out, "{}", format!("--- {old_label}\n").red().bold());
  let _ = write!(out, "{}", format!("+++ {new_label}\n").green().bold());

  for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
    let _ = write!(out, "{}", format!("{}\n", hunk.header()).cyan());
    for change in hunk.iter_changes() {
      match change.tag() {
        ChangeTag::Delete => {
          let _ = write!(out, "{}", format!("-{change}").red());
        }
        ChangeTag::Insert => {
          let _ = write!(out, "{}", format!("+{change}").green());
        }
        ChangeTag::Equal => {
          let _ = write!(out, " {change}");
        }
      }
    }
  }

  out
}
