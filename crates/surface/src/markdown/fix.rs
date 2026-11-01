//! Fixes the markdownlint findings whose intended fix is unambiguous.
//!
//! markdownlint finds the problems and this module rewrites only the lines
//! they name. It owns which rules are safe to fix and when. markdownlint's
//! own fixers stay off, since some guess wrong, such as MD018 turning a
//! wrapped line that starts `#123` into a heading.

use std::collections;
use std::fs;
use std::path;

/// Applies the safe fixes for `problems` and returns the files it changed.
///
/// # Errors
///
/// Returns an error when a file cannot be read or written.
pub fn apply(
  problems: &[crate::Problem],
) -> Result<Vec<path::PathBuf>, crate::Error> {
  let mut by_file: collections::BTreeMap<&path::Path, Vec<&crate::Problem>> =
    collections::BTreeMap::new();
  for problem in problems {
    by_file.entry(&problem.path).or_default().push(problem);
  }
  let mut changed = Vec::new();
  for (path, mut found) in by_file {
    let access = |source| crate::Error::File {
      path: path.to_owned(),
      source,
    };
    let text = fs::read_to_string(path).map_err(access)?;
    let mut lines: Vec<String> =
      text.split_inclusive('\n').map(str::to_owned).collect();
    let levels = heading_levels(&lines);
    // Right to left, so a fix does not move the columns of those before it.
    found
      .sort_by_key(|problem| std::cmp::Reverse((problem.line, problem.column)));
    let mut edited = false;
    for problem in found {
      let Some(at) = usize::try_from(problem.line)
        .ok()
        .and_then(|n| n.checked_sub(1))
      else {
        continue;
      };
      if let Some(line) = fix(problem, &lines, at, &levels) {
        lines[at] = line;
        edited = true;
      }
    }
    if edited {
      fs::write(path, lines.concat()).map_err(access)?;
      changed.push(path.to_owned());
    }
  }
  Ok(changed)
}

/// The line `problem` names, at index `at` of `lines`, with the problem
/// fixed, or `None` when its rule has no safe fix there.
fn fix(
  problem: &crate::Problem,
  lines: &[String],
  at: usize,
  levels: &[usize],
) -> Option<String> {
  let line = lines.get(at)?;
  let body = line.trim_end_matches(['\r', '\n']);
  let end = &line[body.len()..];
  // markdownlint names a rule `MD040/fenced-code-language`.
  let rule = problem.rule.as_deref()?.split('/').next()?;
  let fixed = match rule {
    // A fence with no language gets `text`.
    "MD040" => format!("{}text", body.trim_end()),
    // A bare URL becomes an autolink.
    "MD034" => {
      let url = quoted(&problem.message, "[Context: \"", "\"]")?;
      let column = usize::try_from(problem.column).ok()?.checked_sub(1)?;
      let start = body.char_indices().nth(column)?.0;
      body[start..].starts_with(url).then_some(())?;
      let rest = &body[start + url.len()..];
      format!("{}<{url}>{rest}", &body[..start])
    }
    // A heading drops a trailing `.`, `,`, `:` or `;`. A `!` stays, since
    // it carries tone.
    "MD026" => {
      let mark = quoted(&problem.message, "[Punctuation: '", "']")?;
      matches!(mark, "." | "," | ":" | ";").then_some(())?;
      body.trim_end().strip_suffix(mark)?.trim_end().to_owned()
    }
    // A paragraph that is only emphasis becomes a heading one level below
    // the one it is under, or level 2 at the top.
    "MD036" => {
      let text = quoted(&problem.message, "[Context: \"", "\"]")?;
      body.starts_with(['*', '_']).then_some(())?;
      // markdownlint already skips text that ends in punctuation; a label
      // such as `Note: unstable` is a callout, not a section.
      (!text.contains(':')).then_some(())?;
      let level = (levels.get(at)? + 1).clamp(2, 6);
      format!("{} {text}", "#".repeat(level))
    }
    // `#Title` gets its space only when it stands alone between blank
    // lines and starts with a letter, so `#123` in prose stays prose.
    "MD018" => {
      let hashes = body.len() - body.trim_start_matches('#').len();
      let rest = &body[hashes..];
      rest.chars().next().filter(|c| c.is_alphabetic())?;
      let blank =
        |line: Option<&String>| line.is_none_or(|l| l.trim().is_empty());
      let alone = blank(at.checked_sub(1).and_then(|i| lines.get(i)))
        && blank(lines.get(at + 1));
      alone.then_some(())?;
      format!("{} {rest}", &body[..hashes])
    }
    _ => return None,
  };
  Some(format!("{fixed}{end}"))
}

/// The text in `message` between `open` and the last `close`.
fn quoted<'a>(message: &'a str, open: &str, close: &str) -> Option<&'a str> {
  let start = message.find(open)? + open.len();
  let end = message.rfind(close)?;
  message.get(start..end)
}

/// For each line, the level of the last ATX heading at or above it outside
/// code fences, or 0 when there is none.
fn heading_levels(lines: &[String]) -> Vec<usize> {
  let mut fenced = false;
  let mut level = 0;
  lines
    .iter()
    .map(|line| {
      let text = line.trim_start();
      if text.starts_with("```") || text.starts_with("~~~") {
        fenced = !fenced;
      } else if !fenced {
        let hashes = text.len() - text.trim_start_matches('#').len();
        let rest = &text[hashes..];
        if (1..=6).contains(&hashes)
          && (rest.is_empty() || rest.starts_with([' ', '\t']))
        {
          level = hashes;
        }
      }
      level
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path;

  use super::*;
  use crate::testing;

  fn problem(
    path: &path::Path,
    line: u32,
    column: u32,
    message: &str,
  ) -> crate::Problem {
    crate::Problem {
      path: path.to_owned(),
      line,
      column,
      message: message.to_owned(),
      rule: super::super::rule(message).map(str::to_owned),
    }
  }

  #[test]
  fn apply_fixes_only_what_cannot_change_meaning() {
    let source = "# a\n\n**Bold heading**\n\nSee https://example.com/x, ok.\n\n\
                  ## Done.\n\n```\ncode\n```\n\n#Title\n\nSee PR\n#123 here.\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let path = dir.path().join("a.md");
    let found = [
      problem(
        &path,
        3,
        1,
        "error MD036/no-emphasis-as-heading x [Context: \"Bold heading\"]",
      ),
      problem(
        &path,
        5,
        5,
        "error MD034/no-bare-urls x [Context: \"https://example.com/x\"]",
      ),
      problem(
        &path,
        7,
        8,
        "error MD026/no-trailing-punctuation x [Punctuation: '.']",
      ),
      problem(
        &path,
        9,
        1,
        "error MD040/fenced-code-language x [Context: \"```\"]",
      ),
      problem(
        &path,
        13,
        1,
        "error MD018/no-missing-space-atx x [Context: \"#Title\"]",
      ),
      problem(
        &path,
        16,
        1,
        "error MD018/no-missing-space-atx x [Context: \"#123 here.\"]",
      ),
    ];
    let changed = apply(&found).expect("apply");
    assert_eq!(changed, std::slice::from_ref(&path));
    let text = fs::read_to_string(&path).expect("read");
    let want = "# a\n\n## Bold heading\n\nSee <https://example.com/x>, ok.\n\n\
                ## Done\n\n```text\ncode\n```\n\n# Title\n\nSee PR\n#123 here.\n";
    assert_eq!(text, want);
  }

  #[test]
  fn apply_leaves_what_it_cannot_be_sure_of() {
    let source = "# a\n\n**Note: unstable**\n\n## Wow!\n\nSee x https://a.b ok.\n\n\
                  #123\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let path = dir.path().join("a.md");
    let found = [
      problem(&path, 3, 1, "error MD036/x x [Context: \"Note: unstable\"]"),
      problem(&path, 5, 7, "error MD026/x x [Punctuation: '!']"),
      // The URL is not where markdownlint says, so the line is left alone.
      problem(&path, 7, 1, "error MD034/x x [Context: \"https://a.b\"]"),
      problem(&path, 9, 1, "error MD018/x x [Context: \"#123\"]"),
    ];
    assert!(apply(&found).expect("apply").is_empty());
    assert_eq!(fs::read_to_string(&path).expect("read"), source);
  }
}
