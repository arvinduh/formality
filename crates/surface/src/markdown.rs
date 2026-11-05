//! Markdown, formatted in-process by dprint and linted by markdownlint-cli2.
//!
//! dprint formats block-level HTML and writes hard breaks as a backslash on
//! its own. Fenced code in a language fml knows is formatted by that
//! language; other code is kept as written. markdownlint only reports, and
//! `fix` applies the fixes whose intent is clear itself.

mod fence;
mod fix;

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
    // The first pass formats everything but the code blocks and collects
    // them; the second, for files that have any, writes them back formatted.
    let mut blocks = fence::Blocks::new();
    let mut passes = Vec::new();
    for file in files {
      let text =
        fs::read_to_string(file).map_err(|source| crate::Error::File {
          path: file.clone(),
          source,
        })?;
      let mut fenced = false;
      let tidy = markdown(file, &text, &config, |tag, code| {
        if let Some(extension) = fence::extension(tag) {
          let codes = blocks.entry(extension).or_default();
          codes
            .entry(code.to_owned())
            .or_insert_with(|| code.to_owned());
          fenced = true;
        }
        None
      })?;
      passes.push((text, tidy, fenced));
    }
    if !blocks.is_empty() {
      fence::format(ctx, &mut blocks)?;
    }
    let mut changed = Vec::new();
    for (file, (text, tidy, fenced)) in files.iter().zip(passes) {
      let tidy = if fenced {
        markdown(file, &text, &config, |tag, code| {
          let extension = fence::extension(tag)?;
          blocks.get(extension)?.get(code).cloned()
        })?
      } else {
        tidy
      };
      let Some(tidy) = tidy else {
        continue;
      };
      if !check {
        fs::write(file, tidy).map_err(|source| crate::Error::File {
          path: file.clone(),
          source,
        })?;
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
    fix: bool,
  ) -> Result<crate::Report, crate::Error> {
    // markdownlint only reports; fml applies the fixes that keep meaning.
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
    let lint = || -> Result<Vec<crate::Problem>, crate::Error> {
      let output = crate::run(&MARKDOWNLINT, &args, files, ctx.root, &[0, 1])?;
      Ok(crate::locate(
        crate::problems(&output.stderr, ctx.root, rule),
        files,
      ))
    };
    let mut problems = lint()?;
    let changed = if fix {
      fix::apply(&problems)?
    } else {
      Vec::new()
    };
    if !changed.is_empty() {
      problems = lint()?;
    }
    drop(rules);
    Ok(crate::Report { changed, problems })
  }
}

/// `text`, the contents of `file`, as dprint formats it, or `None` when it
/// is already formatted. `block` gives a fenced block's formatted code from
/// its info string and code, or `None` to keep it as written.
fn markdown(
  file: &path::Path,
  text: &str,
  config: &configuration::Configuration,
  mut block: impl FnMut(&str, &str) -> Option<String>,
) -> Result<Option<String>, crate::Error> {
  dprint_plugin_markdown::format_text(text, config, |tag, code, _| {
    Ok(block(tag, code))
  })
  .map_err(|source| crate::Error::Markdown {
    path: file.to_owned(),
    source,
  })
}

/// The dprint config that carries `settings`. Prose wraps at the line length,
/// so MD013 only flags lines with no space to break at. Lists indent by their
/// marker's width, or by 4 spaces when `indent_size` is 4 or more. Code blocks
/// keep their indentation, which dprint would otherwise strip.
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
    .text_wrap(configuration::TextWrap::Always)
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
  fn format_wraps_prose_at_the_line_length() {
    let source = format!("# a\n\n{}\n", ["word"; 20].join(" "));
    let dir = testing::workspace(&[("a.md", &source)]);
    let files = [dir.path().join("a.md")];
    testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false).expect("format")
    });
    let text = fs::read_to_string(&files[0]).expect("read");
    let (head, tail) = (["word"; 16].join(" "), ["word"; 4].join(" "));
    assert_eq!(text, format!("# a\n\n{head}\n{tail}\n"), "wrapped at 80");
  }

  #[test]
  fn format_formats_fenced_code_with_its_language() {
    let source = "# a\n\n```rust\nfn main() {\nlet x = 1;\n}\n```\n\n\
                  ```rust,ignore\nlet x =\n```\n\n```text\na   b\n```\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let files = [dir.path().join("a.md")];
    testing::with_context(dir.path(), |ctx| {
      Markdown.format(ctx, &files, false)
    })
    .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    let want = "# a\n\n```rust\nfn main() {\n  let x = 1;\n}\n```\n\n\
                ```rust,ignore\nlet x =\n```\n\n```text\na   b\n```\n";
    assert_eq!(text, want, "a block that does not parse stays as written");
    let left: Vec<_> = fs::read_dir(dir.path()).expect("list").collect();
    assert_eq!(left.len(), 1, "the blocks' temp dir is gone");
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
  fn lint_with_fix_keeps_meaning_and_reports_what_is_left() {
    let source = "# a\n\nSee the fix in PR\n#123 for details.\n\n```\nx\n```\n";
    let dir = testing::workspace(&[("a.md", source)]);
    let files = [dir.path().join("a.md")];
    let report = testing::with_context(dir.path(), |ctx| {
      Markdown.lint(ctx, &files, true).expect("lint")
    });
    let text = fs::read_to_string(&files[0]).expect("read");
    let want =
      "# a\n\nSee the fix in PR\n#123 for details.\n\n```text\nx\n```\n";
    assert_eq!(text, want, "#123 stays prose, not a heading");
    assert_eq!(report.changed, files);
    let rules: Vec<_> = report
      .problems
      .iter()
      .filter_map(|p| p.rule.as_deref())
      .collect();
    assert_eq!(
      rules,
      ["MD018/no-missing-space-atx"],
      "only the unfixed one"
    );
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
