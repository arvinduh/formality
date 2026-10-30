//! JavaScript and TypeScript, formatted and linted by biome.
//!
//! Owns biome's arguments, its GitHub-style report lines, and the lint rules
//! fml applies when biome has no config of its own.

use std::fs;
use std::path;

use config;
use tempfile;
use tool;

/// The biome binary.
const BIOME: tool::Tool = tool::Tool {
  binary: "biome",
  install: "npm install -g @biomejs/biome",
};

/// The config files biome reads on its own.
const CONFIGS: &[&str] = &["biome.json", "biome.jsonc"];

/// The biome config fml lints with when biome has none: the recommended
/// rules, plus the Google TypeScript rules they leave off.
const RULES: &str = r#"{
  "linter": {
    "rules": {
      "recommended": true,
      "style": {
        "noCommonJs": "error",
        "noDefaultExport": "error",
        "noInferrableTypes": "error",
        "noNamespace": "error",
        "useBlockStatements": "error",
        "useConsistentMemberAccessibility": {
          "level": "error",
          "options": { "accessibility": "noPublic" }
        },
        "useConsistentTypeDefinitions": "error",
        "useDefaultSwitchClause": "error",
        "useForOf": "error",
        "useNamingConvention": "error",
        "useNumberNamespace": "error",
        "useSingleVarDeclarator": "error",
        "useThrowOnlyError": "error"
      },
      "suspicious": {
        "noEmptyBlockStatements": "error",
        "noForIn": "error",
        "noVar": "error"
      }
    }
  }
}
"#;

/// JavaScript and TypeScript, with JSX and the module variants.
pub struct JavaScript;

impl crate::Surface for JavaScript {
  fn name(&self) -> &'static str {
    "javascript"
  }

  fn extensions(&self) -> &'static [&'static str] {
    &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"]
  }

  fn format(
    &self,
    ctx: &crate::Context,
    files: &[path::PathBuf],
    check: bool,
  ) -> Result<crate::Report, crate::Error> {
    let native = tool::nearest_config(ctx.root, CONFIGS).is_some();
    let mut args = vec!["format".to_owned(), "--reporter=github".to_owned()];
    args.extend(settings(ctx.settings(self.name(), native)?.as_ref()));
    let output = crate::run(&BIOME, &args, files, ctx.root, &[0, 1])?;
    let problems = reported(&output.stdout, ctx.root).into_iter();
    let changed =
      crate::changed(files, ctx.root, problems.map(|problem| problem.path));
    if !check && !changed.is_empty() {
      args.push("--write".to_owned());
      crate::run(&BIOME, &args, &changed, ctx.root, &[0])?;
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
    let mut args = vec!["lint".to_owned(), "--reporter=github".to_owned()];
    // Held until biome exits: the directory holding fml's rules, when biome
    // has no config of its own.
    let rules = if tool::nearest_config(ctx.root, CONFIGS).is_some() {
      None
    } else {
      let dir = tempfile::tempdir()?;
      fs::write(dir.path().join("biome.json"), RULES)?;
      args.push(format!("--config-path={}", dir.path().display()));
      Some(dir)
    };
    if fix {
      args.push("--write".to_owned());
    }
    let output = crate::run(&BIOME, &args, files, ctx.root, &[0, 1])?;
    drop(rules);
    Ok(crate::Report {
      changed: Vec::new(),
      problems: crate::locate(reported(&output.stdout, ctx.root), files),
    })
  }
}

/// The biome formatter flags that carry `settings`, or none to let biome read
/// its own config.
fn settings(settings: Option<&config::Global>) -> Vec<String> {
  let Some(settings) = settings else {
    return Vec::new();
  };
  let quotes = match settings.quote_style {
    config::Quote::Single => "single",
    config::Quote::Double => "double",
  };
  let ending = match settings.end_of_line {
    config::EndOfLine::Lf => "lf",
    config::EndOfLine::Crlf => "crlf",
  };
  vec![
    "--indent-style=space".to_owned(),
    format!("--indent-width={}", settings.indent_size),
    format!("--line-width={}", settings.line_length),
    format!("--line-ending={ending}"),
    format!("--javascript-formatter-quote-style={quotes}"),
  ]
}

/// Reads biome's GitHub reporter lines,
/// `::level title=rule,file=path,line=n,...,col=n,...::message`.
fn reported(output: &str, cwd: &path::Path) -> Vec<crate::Problem> {
  output
    .lines()
    .filter_map(|line| {
      let (fields, message) = line.strip_prefix("::")?.split_once("::")?;
      let (_, fields) = fields.split_once(' ')?;
      let field = |name: &str| {
        fields
          .split(',')
          .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
      };
      Some(crate::Problem {
        path: cwd.join(unescape(field("file")?)),
        line: field("line")?.parse().ok()?,
        column: field("col")?.parse().ok()?,
        message: format!("{} {}", field("title")?, message.trim()),
      })
    })
    .collect()
}

/// Undoes the escaping GitHub workflow commands apply to property values,
/// such as `%3A` for the colon in a Windows drive letter.
fn unescape(value: &str) -> String {
  [
    ("%3A", ":"),
    ("%2C", ","),
    ("%0D", "\r"),
    ("%0A", "\n"),
    ("%25", "%"),
  ]
  .iter()
  .fold(value.to_owned(), |text, (from, to)| text.replace(from, to))
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::Surface;
  use crate::testing;

  #[test]
  fn reported_paths_lose_their_workflow_command_escapes() {
    assert_eq!(unescape(r"C%3A\a%2Cb%253A.ts"), r"C:\a,b%3A.ts");
  }

  #[test]
  fn format_check_reports_without_writing_and_format_writes() {
    let source = "const a = 'x'\n";
    let dir = testing::workspace(&[("a.ts", source)]);
    let files = [dir.path().join("a.ts")];
    let check = testing::with_context(dir.path(), |ctx| {
      JavaScript.format(ctx, &files, true).expect("check")
    });
    assert_eq!(check.changed, files);
    assert_eq!(fs::read_to_string(&files[0]).expect("read"), source);

    testing::with_context(dir.path(), |ctx| {
      JavaScript.format(ctx, &files, false)
    })
    .expect("format");
    let text = fs::read_to_string(&files[0]).expect("read");
    assert_eq!(text, "const a = \"x\";\n");
  }

  #[test]
  fn lint_uses_fml_rules_only_without_a_biome_config() {
    let source = (
      "a.ts",
      "export function f(): void {\n  for (const k in {}) {\n    console.log(k);\n  }\n}\n",
    );
    let dir = testing::workspace(&[source]);
    let files = [dir.path().join("a.ts")];
    let report = testing::with_context(dir.path(), |ctx| {
      JavaScript.lint(ctx, &files, false).expect("lint")
    });
    let found = |report: &crate::Report| {
      report
        .problems
        .iter()
        .any(|p| p.message.contains("noForIn"))
    };
    assert!(found(&report), "noForIn under fml's rules");
    assert_eq!(report.problems[0].path, files[0]);
    assert_eq!(report.problems[0].line, 2);

    let dir = testing::workspace(&[source, ("biome.json", "{}\n")]);
    let files = [dir.path().join("a.ts")];
    let report = testing::with_context(dir.path(), |ctx| {
      JavaScript.lint(ctx, &files, false).expect("lint")
    });
    assert!(!found(&report), "the biome.json leaves noForIn off");
  }
}
