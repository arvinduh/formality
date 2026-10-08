//! TOML language surface: formats and lints via Taplo.
//!
//! Implements `super::LanguageSurface` for TOML, syncing `taplo.toml`.
//! Fleet registration is owned by `super::registry`.

use std::path;
use std::time;

use crate::config;
use crate::config::facets;
use crate::surfaces;
use crate::surfaces::sync;
use crate::surfaces::sync::native;
use crate::surfaces::tooling;

/// The `taplo.toml` settings `ctx` resolves to.
fn taplo_config(ctx: &surfaces::ExecutionContext) -> native::ToolConfig {
  let toml = ctx.lang_config.toml.as_ref();
  let indent = if ctx.lang_config.use_tabs {
    "\t".to_string()
  } else {
    " ".repeat(ctx.lang_config.indent_size)
  };
  native::ToolConfig::new("taplo.toml")
    .set(
      "formatting.align_entries",
      toml.and_then(|t| t.align_entries).unwrap_or(false),
    )
    .set(
      "formatting.column_width",
      native::int(ctx.lang_config.line_length),
    )
    .set(
      "formatting.indent_entries",
      toml.and_then(|t| t.indent_entries).unwrap_or(false),
    )
    .set("formatting.indent_string", indent)
    .set(
      "formatting.indent_tables",
      toml.and_then(|t| t.indent_tables).unwrap_or(false),
    )
    .set(
      "formatting.crlf",
      ctx.global_config.end_of_line.eq_ignore_ascii_case("crlf"),
    )
}

/// The `[formatting]` keys `taplo format` takes inline through `-o k=v`.
/// `taplo lint` has no inline override, and needs none of them.
const TAPLO_INLINE_KEYS: &[&str] = &[
  "column_width",
  "indent_string",
  "crlf",
  "align_entries",
  "indent_entries",
  "indent_tables",
];

/// The inline `-o` overrides for `taplo format`.
fn taplo_format_args(ctx: &surfaces::ExecutionContext) -> Vec<String> {
  taplo_config(ctx).section("formatting").flagged(
    "-o",
    TAPLO_INLINE_KEYS,
    native::Quote::None,
  )
}

/// Builds argument vector for a `taplo lint` invocation whose output is
/// safe to parse for the LSP server (`fml lsp`, Fixes #159 [pre-recreation], #165 [pre-recreation]). taplo has
/// no JSON/structured reporter reachable by CLI flag — only a
/// codespan-reporting-style human diagnostic block (`error: <message>` then
/// a `┌─ path:line:col` location line, verified against a real taplo v0.10.0
/// run) — so `--colors never` is the only flag needed to make that text
/// output parseable (colored output otherwise interleaves ANSI escapes into
/// the location line).
#[must_use]
pub fn build_taplo_lsp_lint_args(
  files: &[path::PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec![
    "lint".to_string(),
    "--colors".to_string(),
    "never".to_string(),
  ];
  for f in files {
    args.push(taplo_path_arg(&f.to_string_lossy(), cfg!(windows)));
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Spells `path` as a taplo file argument that matches exactly that file.
///
/// taplo globs every file argument, so a path is a pattern: `[ ] { } ( ) * ?`
/// are each wrapped in a one-character class, which both glob engines taplo
/// ships with (Rust `glob` natively, `fast-glob` in the npm build) read as the
/// literal. The npm build's `fast-glob` also reads `\` as an escape, so a
/// Windows path matches nothing and taplo exits 0 having done nothing (Issue
/// #501); `backslash_is_separator` (`cfg!(windows)` in production) turns `\`
/// into `/`, which both engines accept as a Windows separator.
fn taplo_path_arg(path: &str, backslash_is_separator: bool) -> String {
  let mut arg = String::with_capacity(path.len());
  for c in path.chars() {
    match c {
      '\\' if backslash_is_separator => arg.push('/'),
      '[' | ']' | '{' | '}' | '(' | ')' | '*' | '?' => {
        arg.push('[');
        arg.push(c);
        arg.push(']');
      }
      _ => arg.push(c),
    }
  }
  arg
}

/// TOML language surface implementation.
#[derive(Debug, Default)]
pub struct TomlSurface;

impl facets::DeclaresFacets for TomlSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs
      | facets::Facet::IndentWidth
      | facets::Facet::LineLength => facets::FacetSupport::Configurable,
      facets::Facet::QuoteStyle
      | facets::Facet::TrailingComma
      | facets::Facet::ImportSort
      | facets::Facet::ProseWrap
      | facets::Facet::Edition
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

const TOML_EXTENSIONS: &[&str] = &["toml"];

impl surfaces::LanguageSurface for TomlSurface {
  fn name(&self) -> &'static str {
    "toml"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["taplo"]
  }

  fn aliases(&self) -> &[&'static str] {
    &[]
  }

  fn file_extensions(&self) -> &[&'static str] {
    TOML_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(Self)
  }

  fn marker_files(&self) -> &[&'static str] {
    &["taplo.toml", ".taplo.toml"]
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![surfaces::ToolInfo {
      binary: "taplo",
      description: "TOML toolkit, formatter and linter",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    let files =
      match ctx.files_for(self.name(), "taplo", TOML_EXTENSIONS, start) {
        Ok(files) => files,
        Err(res) => return res,
      };

    // Inline `-o key=value` instead of writing `taplo.toml` to disk — see
    // `taplo_format_args` (Fixes #151 [pre-recreation]). `fml sync` remains the
    // only path that materializes the file.
    let inline_config = taplo_format_args(ctx);

    if ctx.check_only {
      return sync::diff_check_via_tempcopy(
        &files,
        |scratch| {
          let mut cmd = ctx.command("taplo");
          cmd
            .arg("format")
            .args(&inline_config)
            .arg(taplo_path_arg(&scratch.to_string_lossy(), cfg!(windows)));
          cmd.args(ctx.lang_config.tool_args("taplo"));
          cmd.output()
        },
        self.name(),
        start,
      );
    }

    let mut cmd = ctx.command("taplo");
    cmd.arg("format");
    cmd.args(&inline_config);

    for f in &files {
      cmd.arg(taplo_path_arg(&f.to_string_lossy(), cfg!(windows)));
    }

    cmd.args(ctx.lang_config.tool_args("taplo"));

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if fix {
      return tooling::lint_fix_unsupported(self.name(), start);
    }

    let files =
      match ctx.files_for(self.name(), "taplo", TOML_EXTENSIONS, start) {
        Ok(files) => files,
        Err(res) => return res,
      };

    let mut cmd = ctx.command("taplo");
    cmd.arg("lint");

    for f in &files {
      cmd.arg(taplo_path_arg(&f.to_string_lossy(), cfg!(windows)));
    }

    cmd.args(ctx.lang_config.tool_args("taplo"));

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt` no longer goes through this path (Fixes #151 [pre-recreation]): it passes the
  // resolved config to taplo inline via repeated `-o key=value` flags (see
  // `taplo_format_args`, used in `format()` above). This method
  // is now reached only by `fml sync`, for users who explicitly want
  // `taplo.toml` materialized on disk.
  fn sync_config(
    &self,
    ctx: &surfaces::ExecutionContext,
    check: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();
    taplo_config(ctx).sync(ctx, check, start, self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::config::facets::DeclaresFacets;
  use crate::surfaces;
  use crate::surfaces::LanguageSurface;

  #[test]
  fn toml_surface_facets() {
    let surface = TomlSurface;
    assert_eq!(
      surface.facet_support(facets::Facet::IndentTabs),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::IndentWidth),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::LineLength),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::QuoteStyle),
      facets::FacetSupport::Unsupported
    );
  }

  #[test]
  fn toml_surface_tool_info() {
    let surface = TomlSurface;
    let cfg = config::ResolvedLangConfig::new("toml");
    let tools = surface.tool_info(&cfg);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].binary, "taplo");
    assert!(tools[0].is_required_for_fmt);
    assert!(tools[0].is_required_for_lint);
  }

  /// Options default to off and flow through when set; indentation, width
  /// and CRLF reach both the file and the inline `-o` flags unprefixed.
  #[test]
  fn taplo_config_table() {
    let temp = tempfile::TempDir::new().unwrap();
    let on = config::options::TomlOptions {
      align_entries: Some(true),
      indent_entries: Some(true),
      indent_tables: Some(true),
    };
    // (options, indent, width, eol, expected inline args)
    let cases = [
      (
        None,
        2,
        80,
        "lf",
        vec![
          "column_width=80",
          "indent_string=  ",
          "crlf=false",
          "align_entries=false",
          "indent_entries=false",
          "indent_tables=false",
        ],
      ),
      (
        Some(on),
        4,
        100,
        "crlf",
        vec![
          "column_width=100",
          "indent_string=    ",
          "crlf=true",
          "align_entries=true",
          "indent_entries=true",
          "indent_tables=true",
        ],
      ),
    ];
    for (toml, indent, width, eol, expected) in cases {
      let mut lang = config::ResolvedLangConfig::new("toml");
      lang.toml = toml;
      lang.indent_size = indent;
      lang.line_length = width;
      let mut ctx = surfaces::test_ctx(temp.path(), lang);
      ctx.global_config = std::sync::Arc::new(config::ResolvedGlobalConfig {
        end_of_line: eol.to_string(),
        ..Default::default()
      });
      let args = taplo_format_args(&ctx);
      let values: Vec<&str> =
        args.iter().skip(1).step_by(2).map(String::as_str).collect();
      assert_eq!(values, expected);
      assert!(args.iter().step_by(2).all(|flag| flag == "-o"));
      let file = taplo_config(&ctx).render();
      assert!(file.contains("[formatting]"), "{file}");
      assert!(file.contains(&format!("column_width = {width}")), "{file}");
    }
  }

  #[test]
  fn toml_sync_config() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = TomlSurface;
    let mut lang_cfg = config::ResolvedLangConfig::new("toml");
    lang_cfg.line_length = 80;
    lang_cfg.indent_size = 2;

    let ctx = surfaces::test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert_eq!(res.status.created_file_names(), ["taplo.toml"]);

    let config_path = temp.path().join("taplo.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("column_width = 80"));
    assert!(content.contains("indent_string = \"  \""));
  }

  #[test]
  fn toml_extra_args_propagation() {
    let mut cmd = tooling::create_tool_command("taplo");
    cmd.arg("format").arg("-");
    let extra_args = vec!["--colors".to_string(), "never".to_string()];
    cmd.args(&extra_args);
    let args: Vec<String> = cmd
      .get_args()
      .map(|a| a.to_string_lossy().to_string())
      .collect();
    assert_eq!(args, vec!["format", "-", "--colors", "never"]);
  }

  #[test]
  fn taplo_path_arg_windows_path_uses_forward_slashes() {
    assert_eq!(
      taplo_path_arg(r"C:\Users\RUNNER~1\Temp\fml-check-x\0\a.toml", true),
      "C:/Users/RUNNER~1/Temp/fml-check-x/0/a.toml"
    );
  }

  #[test]
  fn taplo_path_arg_keeps_backslash_where_it_is_a_name_char() {
    assert_eq!(taplo_path_arg(r"/srv/a\b.toml", false), r"/srv/a\b.toml");
  }

  #[test]
  fn taplo_path_arg_escapes_glob_metacharacters() {
    assert_eq!(
      taplo_path_arg(r"C:\w\a[b]\{c}(d)*?!.toml", true),
      "C:/w/a[[]b[]]/[{]c[}][(]d[)][*][?]!.toml"
    );
    assert_eq!(
      taplo_path_arg("/w/a[b]/c{d}.toml", false),
      "/w/a[[]b[]]/c[{]d[}].toml"
    );
  }

  #[test]
  fn toml_format_writes_file_under_glob_metacharacter_dir() {
    if !tooling::check_binary_exists("taplo") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    // Unescaped, `[b]` matches only `b` (Rust `glob`) and `(c){d,e}` is a
    // group and an alternation (`fast-glob`): no engine finds this file.
    let dir = temp.path().join("a[b](c){d,e}");
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("f.toml");
    std::fs::write(&file, "[package]\n name =   \"x\"\n").unwrap();

    let ctx = surfaces::test_ctx_with_paths(
      temp.path(),
      config::ResolvedLangConfig::new("toml"),
      vec![file.clone()],
    );
    let res = TomlSurface.format(&ctx);

    assert!(
      matches!(res.status, surfaces::SurfaceStatus::Passed),
      "{:?}",
      res.status
    );
    assert_eq!(
      std::fs::read_to_string(&file).unwrap(),
      "[package]\nname = \"x\"\n"
    );
  }

  #[test]
  fn toml_lint_reads_file_under_glob_metacharacter_dir() {
    if !tooling::check_binary_exists("taplo") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().join("a[b](c){d,e}");
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("f.toml");
    std::fs::write(&file, "a = [\n").unwrap();

    let ctx = surfaces::test_ctx_with_paths(
      temp.path(),
      config::ResolvedLangConfig::new("toml"),
      vec![file],
    );
    let res = TomlSurface.lint(&ctx, false);

    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ViolationsFound { .. }),
      "{:?}",
      res.status
    );
  }

  #[test]
  fn test_build_taplo_lsp_lint_args() {
    let files = vec![path::PathBuf::from("a.toml")];
    let args = build_taplo_lsp_lint_args(&files, &[]);
    assert_eq!(
      args,
      vec![
        "lint".to_string(),
        "--colors".to_string(),
        "never".to_string(),
        "a.toml".to_string(),
      ]
    );
  }

  #[test]
  fn build_taplo_lsp_lint_args_spells_path_as_taplo_pattern() {
    let files = vec![path::PathBuf::from(r"C:\w\a[b]\c.toml")];
    let args = build_taplo_lsp_lint_args(&files, &[]);
    let expected = if cfg!(windows) {
      "C:/w/a[[]b[]]/c.toml"
    } else {
      r"C:\w\a[[]b[]]\c.toml"
    };
    assert_eq!(args[3], expected);
  }

  #[test]
  fn toml_format_does_not_write_taplo_toml() {
    // Fixes #151 [pre-recreation]: `fml fmt` must not write `taplo.toml` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("taplo") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.toml"), "a=1\n").unwrap();

    let surface = TomlSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("toml"));

    let _ = surface.format(&ctx);

    assert!(!temp.path().join("taplo.toml").exists());
    assert!(!temp.path().join(".taplo.toml").exists());
  }

  #[test]
  fn toml_format_check_large_file_no_deadlock() {
    // Fixes #22: formatting large TOML files (>128 KB) in check mode must not deadlock.
    if !tooling::check_binary_exists("taplo") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let file_path = temp.path().join("large_unformatted.toml");

    let mut large_content = String::with_capacity(180_000);
    for i in 0..8000 {
      use std::fmt::Write;
      let _ = writeln!(large_content, "key_{i}=\"value_{i}\"");
    }
    assert!(large_content.len() > 128 * 1024);
    std::fs::write(&file_path, &large_content).unwrap();

    let surface = TomlSurface;
    let mut ctx = surfaces::test_ctx_with_paths(
      temp.path(),
      config::ResolvedLangConfig::new("toml"),
      vec![file_path],
    );
    ctx.check_only = true;

    let res = surface.format(&ctx);
    let surfaces::SurfaceStatus::ViolationsFound { diff, .. } = res.status
    else {
      panic!(
        "expected formatting violations for unformatted TOML, got {:?}",
        res.status
      );
    };
    let diff_str = diff.expect("diff should be present");
    assert!(diff_str.contains("key_0"));

    // Check mode on already-formatted >128 KB file must also complete cleanly and return Passed.
    let formatted_path = temp.path().join("large_formatted.toml");
    let mut formatted_content = String::with_capacity(180_000);
    for i in 0..8000 {
      use std::fmt::Write;
      let _ = writeln!(formatted_content, "key_{i} = \"value_{i}\"");
    }
    assert!(formatted_content.len() > 128 * 1024);
    std::fs::write(&formatted_path, &formatted_content).unwrap();

    let mut ctx_formatted = surfaces::test_ctx_with_paths(
      temp.path(),
      config::ResolvedLangConfig::new("toml"),
      vec![formatted_path],
    );
    ctx_formatted.check_only = true;

    let res_formatted = surface.format(&ctx_formatted);
    assert!(
      matches!(res_formatted.status, surfaces::SurfaceStatus::Passed),
      "expected Passed for formatted TOML, got {:?}",
      res_formatted.status
    );
  }

  #[test]
  fn toml_sync_config_with_alignment_options() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = TomlSurface;
    let mut lang_cfg = config::ResolvedLangConfig::new("toml");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.toml = Some(config::options::TomlOptions {
      align_entries: Some(true),
      indent_entries: Some(true),
      indent_tables: Some(true),
    });

    let ctx = surfaces::test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert_eq!(res.status.created_file_names(), ["taplo.toml"]);

    let config_path = temp.path().join("taplo.toml");
    assert!(config_path.is_file());

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("align_entries = true"));
    assert!(content.contains("indent_entries = true"));
    assert!(content.contains("indent_tables = true"));
    assert!(content.contains("column_width = 100"));
    assert!(content.contains("indent_string = \"    \""));
  }
}
