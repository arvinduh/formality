//! Markdown, formatted in-process by dprint and linted by markdownlint-cli2.
//!
//! dprint formats block-level HTML and writes hard breaks as a backslash on
//! its own. Code blocks are kept as written. markdownlint only reports.

use std::fs;
use std::path;

use config;
use dprint_core;
use dprint_plugin_markdown;
use dprint_plugin_markdown::configuration;
use tempfile;
use tool;

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

impl crate::Surface for Markdown {
  fn name(&self) -> &'static str {
    "markdown"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["md"]
  }

  fn format(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<crate::Report, crate::Error> {
    let settings = ctx.settings(self.name(), false)?.unwrap_or_default();
    let config = dprint(&settings);
    let mut changed = Vec::new();
    for file in files {
      let access = |source| crate::Error::File {
        path: file.clone(),
        source,
      };
      let text = fs::read_to_string(file).map_err(access)?;
      let tidy =
        dprint_plugin_markdown::format_text(&text, &config, |_, _, _| Ok(None))
          .map_err(|source| crate::Error::Markdown {
            path: file.clone(),
            source,
          })?;
      let Some(tidy) = tidy else {
        continue;
      };
      if !check {
        fs::write(file, tidy).map_err(access)?;
      }
      changed.push(file.clone());
    }
    Ok(crate::Report {
      changed,
      problems: Vec::new(),
    })
  }

  fn lint(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    _fix: bool,
  ) -> Result<crate::Report, crate::Error> {
    // markdownlint only reports. Its fixers change meaning, such as MD018
    // turning a wrapped line that starts `#123` into a heading.
    let mut args = Vec::new();
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
    Ok(crate::Report {
      changed: Vec::new(),
      problems: crate::locate(
        crate::problems(&output.stderr, ctx.root, rule),
        files,
      ),
    })
  }
}

/// The dprint config that carries `settings`. Lists indent by their marker's
/// width, or by 4 spaces when `indent_size` is 4 or more. Code blocks keep
/// their indentation, which dprint would otherwise strip.
fn dprint(settings: &config::Global) -> configuration::Configuration {
  let newline = match settings.end_of_line {
    config::EndOfLine::Lf => dprint_core::configuration::NewLineKind::LineFeed,
    config::EndOfLine::Crlf => {
      dprint_core::configuration::NewLineKind::CarriageReturnLineFeed
    }
  };
  let lists = if settings.indent_size >= 4 {
    configuration::ListIndentKind::PythonMarkdown
  } else {
    configuration::ListIndentKind::CommonMark
  };
  configuration::ConfigurationBuilder::new()
    .line_width(u32::from(settings.line_length))
    .new_line_kind(newline)
    .list_indent_kind(lists)
    .code_block_preserve_indentation(true)
    .build()
}

/// The rule markdownlint-cli2 names after the level:
/// `error MD033/no-inline-html ...`.
fn rule(message: &str) -> Option<&str> {
  message
    .split_whitespace()
    .find(|word| word.starts_with("MD"))
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
  use crate::Surface;
  use crate::testing;

  #[test]
  fn format_keeps_code_block_indentation() {
    let source = "# a\n\n```text\n   x\n  y\n```\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false).expect("format")
    });
    assert!(report.changed.is_empty(), "the block is already formatted");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, source);
  }

  #[test]
  fn format_writes_with_dprint() {
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
  fn lint_with_fix_never_rewrites_the_file() {
    let source = "# a\n\nSee the fix in PR\n#123 for details.\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let files = [dir.path().join("a.md")];
    testing::with_context(dir.path(), |ctx| {
      Markdown.lint(ctx, &files, true).expect("lint")
    });
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, source, "#123 stays prose, not a heading");
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
    assert_eq!(check.changed, files, "check reports the trailing spaces");

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
    let want = "<p align=\"center\">\n  <img src=\"a.png\" alt=\"b\">\n</p>\n\n\
                a <b>c</b> d\n";
    assert_eq!(text, want);
  }
}
