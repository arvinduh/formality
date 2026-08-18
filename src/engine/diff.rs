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
  let mut out = String::new();

  out.push_str(&format!("--- {old_label}\n").red().bold().to_string());
  out.push_str(&format!("+++ {new_label}\n").green().bold().to_string());

  for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
    out.push_str(&format!("{}\n", hunk.header()).cyan().to_string());
    for change in hunk.iter_changes() {
      match change.tag() {
        ChangeTag::Delete => {
          out.push_str(&format!("-{change}").red().to_string());
        }
        ChangeTag::Insert => {
          out.push_str(&format!("+{change}").green().to_string());
        }
        ChangeTag::Equal => {
          let _ = write!(out, " {change}");
        }
      }
    }
  }

  out
}
