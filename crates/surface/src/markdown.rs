//! Markdown, formatted by prettier and linted by markdownlint-cli2.
//!
//! After prettier, fml formats block-level HTML, which prettier keeps as
//! written, and under fml's settings writes hard breaks as a backslash.

mod html;

use std::fs;
use std::path;

use config;
use pulldown_cmark;
use tempfile;
use tool;

use crate::Context;
use crate::Error;
use crate::Report;
use crate::Surface;
use crate::prettier;

/// The markdownlint-cli2 binary.
const MARKDOWNLINT: tool::Tool = tool::Tool {
  binary: "markdownlint-cli2",
  install: "npm install -g markdownlint-cli2",
};

/// The config files markdownlint-cli2 reads on its own.
const CONFIGS: &[&str] = &[
  ".markdownlint-cli2.jsonc",
  ".markdownlint-cli2.yaml",
  ".markdownlint-cli2.cjs",
  ".markdownlint-cli2.mjs",
  ".markdownlint.jsonc",
  ".markdownlint.json",
  ".markdownlint.yaml",
  ".markdownlint.yml",
  ".markdownlint.cjs",
  ".markdownlint.mjs",
];

/// Markdown: `.md` files.
pub struct Markdown;

impl Surface for Markdown {
  fn name(&self) -> &'static str {
    "markdown"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["md"]
  }

  fn format(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<Report, Error> {
    let mut changed = prettier::format(self.name(), ctx, files, check)?;
    let args = prettier::args(self.name(), ctx)?;
    let breaks = ctx.settings(self.name(), prettier::native(ctx))?.is_some();
    // Under `check`, this reads the file as written, not as prettier would
    // leave it. A file prettier would change is already listed, so only a
    // file prettier leaves alone can be added here, and for it the two are
    // the same.
    for file in files {
      let text = fs::read_to_string(file)?;
      let mut tidy = html::format(&text, file, ctx.root, &args)?;
      if breaks {
        tidy = backslash_breaks(&tidy);
      }
      if tidy == text {
        continue;
      }
      if !check {
        fs::write(file, &tidy)?;
      }
      if !changed.contains(file) {
        changed.push(file.clone());
      }
    }
    Ok(Report {
      changed,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    ctx: &Context,
    files: &[path::PathBuf],
    fix: bool,
  ) -> Result<Report, Error> {
    let mut args = if fix {
      vec!["--fix".to_owned()]
    } else {
      Vec::new()
    };
    // Held until markdownlint-cli2 exits: the directory holding fml's rules,
    // when it has no config of its own.
    let rules = if tool::nearest_config(ctx.root, CONFIGS).is_some() {
      None
    } else {
      let dir = tempfile::tempdir()?;
      let path = dir.path().join(".markdownlint.jsonc");
      let settings = ctx.settings(self.name(), false)?.unwrap_or_default();
      fs::write(&path, rules(&settings))?;
      args.extend(["--config".to_owned(), path.display().to_string()]);
      Some(dir)
    };
    let output = crate::run(&MARKDOWNLINT, &args, files, ctx.root, &[0, 1])?;
    drop(rules);
    Ok(Report {
      changed: Vec::new(),
      problems: crate::problems(&output.stderr, ctx.root),
    })
  }
}

/// Rewrites each hard break written as trailing spaces as a backslash, which
/// survives editors that trim trailing whitespace.
fn backslash_breaks(src: &str) -> String {
  let mut out = String::with_capacity(src.len());
  let mut cursor = 0;
  for (event, range) in html::parse(src).into_offset_iter() {
    if event == pulldown_cmark::Event::HardBreak
      && src[range.clone()].starts_with(' ')
    {
      out.push_str(&src[cursor..range.start]);
      out.push_str("\\\n");
      cursor = range.end;
    }
  }
  out.push_str(&src[cursor..]);
  out
}

/// The markdownlint config fml lints with when markdownlint has none: every
/// rule, with lists and lines sized like the formatter's, minus the rules
/// prettier already enforces or whose fixers break lists.
fn rules(settings: &config::Global) -> String {
  format!(
    r#"{{
  "default": true,
  "MD007": {{ "indent": {indent} }},
  "MD010": false,
  "MD013": {{ "line_length": {width}, "code_blocks": false, "tables": false }},
  "MD029": false,
  "MD031": false,
  "MD032": false,
  "MD033": false
}}
"#,
    indent = settings.indent_size,
    width = settings.line_length,
  )
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::testing;

  #[test]
  fn format_writes_with_prettier() {
    let dir = testing::workspace(&[("a.md", "# a\n\n* b\n")]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false).expect("format")
    });
    assert_eq!(report.changed, files);
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "# a\n\n- b\n");
  }

  #[test]
  fn lint_reports_markdownlint_findings() {
    let dir = testing::workspace(&[("a.md", "# a\n# b\n")]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.lint(ctx, &files, false).expect("lint")
    });
    assert!(!report.problems.is_empty());
    assert_eq!(report.problems[0].path, files[0]);
    assert!(report.problems[0].message.contains("MD"));
  }

  #[test]
  fn lint_uses_fml_rules_only_without_a_markdownlint_config() {
    let source = ("a.md", "# a\n\n<b>bold</b>\n");
    let dir = testing::workspace(&[source]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.lint(ctx, &files, false).expect("lint")
    });
    assert!(report.problems.is_empty(), "fml's rules allow inline HTML");

    let dir = testing::workspace(&[source, (".markdownlint.json", "{}\n")]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.lint(ctx, &files, false).expect("lint")
    });
    let found = report.problems.iter().any(|p| p.message.contains("MD033"));
    assert!(found, "markdownlint's defaults flag inline HTML");
  }

  #[test]
  fn format_writes_hard_breaks_as_a_backslash() {
    let source = "a  \nb\n\n- c  \n  d\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let files = [dir.path().join("a.md")];
    let check = testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, true).expect("check")
    });
    assert_eq!(check.changed, files, "prettier keeps the spaces");

    testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false)
    })
    .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "a\\\nb\n\n- c\\\n  d\n");
  }

  #[test]
  fn format_formats_block_html_but_not_inline_html() {
    let source = "<p align=\"center\">\n<img src=\"a.png\"     alt=\"b\">\n</p>\n\n\
                  a <b>c</b>     d\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let files = [dir.path().join("a.md")];
    testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false)
    })
    .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    let want = "<p align=\"center\">\n  <img src=\"a.png\" alt=\"b\" />\n</p>\n\n\
                a <b>c</b> d\n";
    assert_eq!(text, want);
  }
}
