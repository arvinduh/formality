use colored::Colorize;
use similar::{ChangeTag, TextDiff};

/// Generates a colored unified diff string between `old_content` and `new_content`.
pub fn render_diff(
  old_content: &str,
  new_content: &str,
  old_label: &str,
  new_label: &str,
) -> String {
  let diff = TextDiff::from_lines(old_content, new_content);
  let mut out = String::new();

  out.push_str(&format!("--- {}\n", old_label).red().bold().to_string());
  out.push_str(&format!("+++ {}\n", new_label).green().bold().to_string());

  for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
    out.push_str(&format!("{}\n", hunk.header()).cyan().to_string());
    for change in hunk.iter_changes() {
      match change.tag() {
        ChangeTag::Delete => {
          out.push_str(&format!("-{}", change).red().to_string());
        }
        ChangeTag::Insert => {
          out.push_str(&format!("+{}", change).green().to_string());
        }
        ChangeTag::Equal => {
          out.push_str(&format!(" {}", change));
        }
      }
    }
  }

  out
}
