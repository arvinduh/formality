//! JavaScript/TypeScript language surface: formats and lints via `biome`.
//!
//! Implements `super::LanguageSurface` for JS/TS, syncing `biome.json`.
//! Fleet registration is owned by `super::registry`.

use crate::config;
use crate::config::facets;
use crate::config::facets::DeclaresFacets;
use crate::surfaces;
use crate::surfaces::LanguageSurface;
use crate::surfaces::sync;
use crate::surfaces::sync::native;
use crate::surfaces::tooling;
use std::path;
use std::time;

/// The `biome.json` settings `ctx` resolves to. Import sorting lives under
/// `assist.actions.source.organizeImports` (Biome >= 2.0).
fn biome_config(ctx: &surfaces::ExecutionContext) -> native::ToolConfig {
  let js = ctx.lang_config.javascript.as_ref();
  native::ToolConfig::new("biome.json")
    .set("$schema", "https://biomejs.dev/schemas/2.0.0/schema.json")
    .set("formatter.enabled", true)
    .set(
      "formatter.indentStyle",
      if ctx.lang_config.use_tabs {
        "tab"
      } else {
        "space"
      },
    )
    .set(
      "formatter.indentWidth",
      native::int(ctx.lang_config.indent_size),
    )
    .set(
      "formatter.lineWidth",
      native::int(ctx.lang_config.line_length),
    )
    .set(
      "javascript.formatter.quoteStyle",
      js.and_then(|j| j.quote_style.as_deref())
        .unwrap_or("double"),
    )
    .set(
      "javascript.formatter.trailingCommas",
      js.and_then(|j| j.trailing_comma.as_deref())
        .unwrap_or("all"),
    )
    .set(
      "javascript.formatter.semicolons",
      js.and_then(|j| j.semicolons.as_deref()).unwrap_or("always"),
    )
    .set("assist.enabled", true)
    .set(
      "assist.actions.source.organizeImports",
      if js.and_then(|j| j.organize_imports).unwrap_or(true) {
        "on"
      } else {
        "off"
      },
    )
    .set("linter.enabled", true)
    .set("linter.rules.preset", "recommended")
}

/// The formatting-layout flags `biome check`/`format` take inline. The linter
/// preset and the import-sorting toggle have no single-action flag, so they
/// stay file-only; biome's defaults already match them.
const BIOME_INLINE_FLAGS: &[(&str, &str)] = &[
  ("indent-style", "formatter.indentStyle"),
  ("indent-width", "formatter.indentWidth"),
  ("line-width", "formatter.lineWidth"),
  (
    "javascript-formatter-quote-style",
    "javascript.formatter.quoteStyle",
  ),
  ("trailing-commas", "javascript.formatter.trailingCommas"),
  ("semicolons", "javascript.formatter.semicolons"),
];

/// JavaScript/TypeScript language surface implementation.
#[derive(Debug, Default)]
pub struct JavaScriptSurface;

impl DeclaresFacets for JavaScriptSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs
      | facets::Facet::IndentWidth
      | facets::Facet::LineLength
      | facets::Facet::QuoteStyle
      | facets::Facet::TrailingComma
      | facets::Facet::ImportSort => facets::FacetSupport::Configurable,
      facets::Facet::ProseWrap
      | facets::Facet::Edition
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

/// Standard file extensions recognized for JavaScript and `TypeScript` source
/// files.
const JS_TS_EXTENSIONS: &[&str] =
  &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

/// The biome flag `fml fmt` passes to keep the linter out of the Smart Format
/// pass, and the value it passes it with. Named because the format path both
/// passes it and refuses an `extra_args` override of it (see
/// [`linter_enabled_override_message`]).
const BIOME_LINTER_ENABLED_FLAG: &str = "--linter-enabled";
/// The value [`BIOME_LINTER_ENABLED_FLAG`] is passed with on the format path.
const BIOME_LINTER_ENABLED_VALUE: &str = "false";

/// Builds the message for an `extra_args` entry that sets biome's
/// `--linter-enabled` on the format path, quoting the offending argument back
/// at the user.
///
/// Fixes #173: the format path always passes `--linter-enabled=false` itself,
/// and biome rejects the flag given twice (`argument --linter-enabled cannot
/// be used multiple times in this context`, exit 1) — reproduced against the
/// pinned `@biomejs/biome@2.5.10`. So *no* spelling in `extra_args` ever
/// worked: `=true` never re-enabled the linter and `=false` never restated a
/// default, because biome refuses the duplicate before it parses either value.
/// Both already produced an `[ERR] Execution error` that was accurate — biome
/// genuinely could not run — but named neither the flag nor `extra_args`. This
/// refusal replaces an opaque tool error with an actionable explanation; it
/// does not correct a misclassified exit code, because there is no lint
/// finding on this path to misclassify. See [`extra_args_set_flag`].
fn linter_enabled_override_message(offending: &str) -> String {
  format!(
    "`[lang.javascript.extra_args] biome` contains `{offending}`, but `fml fmt` \
     already passes `{BIOME_LINTER_ENABLED_FLAG}={BIOME_LINTER_ENABLED_VALUE}` \
     to `biome check --write` — and biome rejects that flag given twice.\n\n\
     No value works here. Because `fml` supplies the flag itself, biome \
     rejects the duplicate before reading either value: it exits with an \
     error, formats nothing, and names neither `extra_args` nor the \
     duplicated flag. `--linter-enabled=true` therefore never re-enables the \
     linter, and `--linter-enabled=false` never restates a default — both \
     simply break the format pass.\n\nRemove \
     `{BIOME_LINTER_ENABLED_FLAG}` from `extra_args` and run linting through \
     `fml lint` (which is where biome's linter belongs) instead."
  )
}

/// Builds the argument list for the "Smart Format" pass: `biome check --write`
/// with the linter disabled so this step only applies formatting and (per
/// `biome.json`'s `organizeImports.enabled`) import sorting — never lint fixes.
/// Linting itself is handled separately by `lint()`.
#[must_use]
fn build_biome_format_args(
  files: &[path::PathBuf],
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec![
    "check".to_string(),
    "--write".to_string(),
    format!("{BIOME_LINTER_ENABLED_FLAG}={BIOME_LINTER_ENABLED_VALUE}"),
  ];
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds argument vector for biome lint invocation.
#[must_use]
fn build_biome_lint_args(
  files: &[path::PathBuf],
  fix: bool,
  extra_args: &[String],
) -> Vec<String> {
  let mut args = vec!["lint".to_string()];
  if fix {
    args.push("--write".to_string());
  }
  if files.is_empty() {
    args.push(".".to_string());
  } else {
    for f in files {
      args.push(f.to_string_lossy().to_string());
    }
  }
  args.extend(extra_args.iter().cloned());
  args
}

/// Builds the argument vector for `biome lint --reporter=json <file>`, used
/// by `fml lsp`'s structured-diagnostics path (Fixes #165 [pre-recreation]) to get
/// machine-readable per-violation output instead of parsing biome's
/// human-readable terminal report. `--reporter=json` is marked experimental
/// by biome as of 2.x but its diagnostic shape (`diagnostics[].location.path`
/// / `.start`/`.end` `{line, column}`, both 1-based) has been stable across
/// the versions this was verified against.
#[must_use]
pub fn build_biome_lint_json_args(file: &path::Path) -> Vec<String> {
  vec![
    "lint".to_string(),
    "--reporter=json".to_string(),
    file.to_string_lossy().to_string(),
  ]
}

impl LanguageSurface for JavaScriptSurface {
  fn name(&self) -> &'static str {
    "javascript"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["biome"]
  }

  fn aliases(&self) -> &[&'static str] {
    &["js", "ts", "typescript", "jsx", "tsx"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    JS_TS_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(Self)
  }

  fn supports_lint_fix(&self) -> bool {
    true
  }

  fn marker_files(&self) -> &[&'static str] {
    &["biome.json", "biome.jsonc", "tsconfig.json", "package.json"]
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![surfaces::ToolInfo {
      binary: "biome",
      description: "Fast formatter and linter for JavaScript, TypeScript, JSX and TSX",
      install_hint: None,
      is_required_for_fmt: true,
      is_required_for_lint: true,
    }]
  }

  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    let files = ctx.matched_files(JS_TS_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    // Fixes #173: refuse, with an explanation, rather than hand biome an
    // argv it rejects with an error that explains nothing. Only checked here,
    // not in `lint()`: `--linter-enabled` is a flag the format path passes
    // itself, and biome's linter is exactly what `fml lint` is supposed to
    // run. Deliberately *above* `tool_missing_guard`: a malformed
    // `formality.toml` is wrong regardless of whether biome happens to be
    // installed, and reporting the config error first is the more useful
    // ordering (it also makes this guard's tests hermetic).
    if let Some(offending) = tooling::extra_args_set_flag(
      BIOME_LINTER_ENABLED_FLAG,
      ctx.lang_config.tool_args("biome"),
    ) {
      return surfaces::SurfaceResult {
        surface_name: self.name(),
        status: surfaces::SurfaceStatus::ExecutionError {
          message: linter_enabled_override_message(&offending),
        },
        duration: start.elapsed(),
      };
    }

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "biome", start, None)
    {
      return res;
    }

    // Inline `--indent-style`/`--line-width`/etc. instead of writing
    // `biome.json` to disk — see `BIOME_INLINE_FLAGS` (Fixes
    // #151 [pre-recreation]). `fml sync` remains the only path that materializes the file.
    let inline_config = biome_config(ctx).flags(BIOME_INLINE_FLAGS);

    if ctx.check_only {
      return sync::diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = tooling::create_tool_command("biome");
          cmd.args(build_biome_format_args(
            &[scratch.to_path_buf()],
            ctx.lang_config.tool_args("biome"),
          ));
          cmd.args(&inline_config);
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
        // `biome check --write` (linter disabled) rewrites the scratch copy
        // in place and exits 0 whether or not it reformatted anything; it
        // only exits non-zero on an operational failure it cannot fix — a
        // parse error, an unreadable file, a bad `--config`. There is no
        // "found drift" exit code on this path (formatting drift is detected
        // by diffing the file), so every non-zero exit is an
        // `ExecutionError` (Fixes #151). Same reasoning applies verbatim to
        // the non-`--check` write branch below (Fixes #155): `biome check
        // --write` has no in-place-write variant of a "found drift" exit
        // code either.
        tooling::classify_all_nonzero_as_error,
      );
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut cmd = tooling::create_tool_command("biome");
    cmd.args(build_biome_format_args(
      &files_to_pass,
      ctx.lang_config.tool_args("biome"),
    ));
    cmd.args(&inline_config);
    cmd.current_dir(ctx.root.as_path());

    tooling::run_tool_command_classified(
      self.name(),
      &mut cmd,
      tooling::classify_all_nonzero_as_error,
    )
  }

  fn lint(
    &self,
    ctx: &surfaces::ExecutionContext,
    fix: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "biome", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(JS_TS_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    let files_to_pass = ctx.files_to_pass(files);

    let mut cmd = tooling::create_tool_command("biome");
    cmd.args(build_biome_lint_args(
      &files_to_pass,
      fix,
      ctx.lang_config.tool_args("biome"),
    ));
    cmd.current_dir(ctx.root.as_path());

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  // `fml fmt` no longer goes through this path for the formatting-layout
  // options (Fixes #151 [pre-recreation]): it passes them to biome inline (see
  // `BIOME_INLINE_FLAGS`, used in `format()` above). The
  // linter preset and `organizeImports` toggle have no equivalent
  // single-action CLI flag (see that function's doc comment for why), so
  // `fml lint` still relies on biome's own defaults there rather than on
  // this file. This method is now reached only by `fml sync`, for users who
  // explicitly want `biome.json` materialized on disk.
  fn sync_config(
    &self,
    ctx: &surfaces::ExecutionContext,
    check: bool,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();
    biome_config(ctx).sync(ctx, check, start, self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config;
  use crate::config::facets;
  use crate::surfaces;
  use std::fs;
  use std::path;

  #[test]
  fn build_biome_format_args_default_and_with_files() {
    let no_files = build_biome_format_args(&[], &[]);
    assert_eq!(
      no_files,
      vec![
        "check".to_string(),
        "--write".to_string(),
        "--linter-enabled=false".to_string(),
        ".".to_string(),
      ]
    );

    let files = vec![path::PathBuf::from("a.ts"), path::PathBuf::from("b.tsx")];
    let extra = vec!["--no-errors-on-unmatched".to_string()];
    let with_files = build_biome_format_args(&files, &extra);
    assert_eq!(
      with_files,
      vec![
        "check".to_string(),
        "--write".to_string(),
        "--linter-enabled=false".to_string(),
        "a.ts".to_string(),
        "b.tsx".to_string(),
        "--no-errors-on-unmatched".to_string(),
      ]
    );
  }

  #[test]
  fn test_build_biome_lint_json_args() {
    let args = build_biome_lint_json_args(path::Path::new("src/a.ts"));
    assert_eq!(
      args,
      vec![
        "lint".to_string(),
        "--reporter=json".to_string(),
        "src/a.ts".to_string(),
      ]
    );
  }

  #[test]
  fn build_biome_lint_args_with_and_without_fix() {
    let no_fix = build_biome_lint_args(&[], false, &[]);
    assert_eq!(no_fix, vec!["lint".to_string(), ".".to_string()]);

    let files = vec![path::PathBuf::from("a.js")];
    let extra = vec!["--max-diagnostics=50".to_string()];
    let with_fix = build_biome_lint_args(&files, true, &extra);
    assert_eq!(
      with_fix,
      vec![
        "lint".to_string(),
        "--write".to_string(),
        "a.js".to_string(),
        "--max-diagnostics=50".to_string(),
      ]
    );
  }

  #[test]
  fn javascript_surface_file_extensions_and_aliases() {
    let surface = JavaScriptSurface;
    assert_eq!(
      surface.file_extensions(),
      &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"]
    );
    assert!(surface.aliases().contains(&"ts"));
    assert!(surface.aliases().contains(&"js"));
    assert!(surface.supports_lint_fix());
  }

  #[test]
  fn javascript_surface_detect() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = JavaScriptSurface;
    assert!(!surfaces::detect_in(&surface, temp.path()));

    let file = temp.path().join("index.ts");
    fs::write(&file, "export const x: number = 1;\n").unwrap();
    assert!(surfaces::detect_in(&surface, temp.path()));
  }

  /// Defaults and options reach both `biome.json` and the inline flags; the
  /// import-sorting toggle is file-only.
  #[test]
  fn biome_config_table() {
    let temp = tempfile::TempDir::new().unwrap();
    let opts = config::options::JavaScriptOptions {
      quote_style: Some("single".to_string()),
      trailing_comma: Some("es5".to_string()),
      semicolons: Some("asNeeded".to_string()),
      organize_imports: Some(false),
    };
    // (options, tabs, expected flags, expected organizeImports)
    let cases = [
      (
        None,
        false,
        [
          "--indent-style=space",
          "--indent-width=4",
          "--line-width=100",
          "--javascript-formatter-quote-style=double",
          "--trailing-commas=all",
          "--semicolons=always",
        ],
        "on",
      ),
      (
        Some(opts),
        true,
        [
          "--indent-style=tab",
          "--indent-width=4",
          "--line-width=100",
          "--javascript-formatter-quote-style=single",
          "--trailing-commas=es5",
          "--semicolons=asNeeded",
        ],
        "off",
      ),
    ];
    for (js, tabs, flags, organize) in cases {
      let mut lang = config::ResolvedLangConfig::new("javascript");
      lang.line_length = 100;
      lang.indent_size = 4;
      lang.use_tabs = tabs;
      lang.javascript = js;
      let cfg = biome_config(&surfaces::test_ctx(temp.path(), lang));
      assert_eq!(cfg.flags(BIOME_INLINE_FLAGS), flags);
      let file: serde_json::Value =
        serde_json::from_str(&cfg.render()).unwrap();
      assert_eq!(
        file["assist"]["actions"]["source"]["organizeImports"],
        organize
      );
      assert_eq!(file["linter"]["rules"]["preset"], "recommended");
      assert_eq!(file["formatter"]["lineWidth"], 100);
    }
  }

  #[test]
  fn javascript_sync_config_from_context() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = JavaScriptSurface;
    let mut lang_cfg = config::ResolvedLangConfig::new("javascript");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.use_tabs = true;
    lang_cfg.javascript = Some(config::options::JavaScriptOptions {
      quote_style: Some("single".to_string()),
      trailing_comma: Some("es5".to_string()),
      semicolons: Some("asNeeded".to_string()),
      organize_imports: Some(true),
    });

    let ctx = surfaces::test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    assert_eq!(res.status.created_file_names(), ["biome.json"]);

    let config_path = temp.path().join("biome.json");
    assert!(config_path.is_file());

    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("\"indentStyle\": \"tab\""));
    assert!(content.contains("\"indentWidth\": 4"));
    assert!(content.contains("\"lineWidth\": 100"));
    assert!(content.contains("\"quoteStyle\": \"single\""));
    assert!(content.contains("\"trailingCommas\": \"es5\""));
    assert!(content.contains("\"assist\""));
    assert!(content.contains("\"organizeImports\": \"on\""));
    assert!(content.contains("\"enabled\": true"));
  }

  #[test]
  fn javascript_facet_declarations() {
    let surface = JavaScriptSurface;
    assert_eq!(
      surface.facet_support(facets::Facet::QuoteStyle),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::TrailingComma),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::ImportSort),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::IndentTabs),
      facets::FacetSupport::Configurable
    );
    assert_eq!(
      surface.facet_support(facets::Facet::ProseWrap),
      facets::FacetSupport::Unsupported
    );
    assert_eq!(
      surface.facet_support(facets::Facet::Standard),
      facets::FacetSupport::Unsupported
    );
  }

  #[test]
  fn javascript_format_does_not_write_biome_json() {
    // Fixes #151 [pre-recreation]: `fml fmt` must not write `biome.json` as a side effect;
    // only `fml sync` should materialize the native config file.
    if !tooling::check_binary_exists("biome") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    fs::write(temp.path().join("a.js"), "const x=1;\n").unwrap();

    let surface = JavaScriptSurface;
    let ctx = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("javascript"),
    );

    let _ = surface.format(&ctx);

    assert!(!temp.path().join("biome.json").exists());
    assert!(!temp.path().join("biome.jsonc").exists());
  }

  #[test]
  fn javascript_check_reports_execution_error_on_formatter_failure() {
    // Fixes #151: when biome cannot format a file on the `fml fmt --check`
    // path (here: a syntax error it has no way to parse or rewrite), the
    // surface must classify that as `ExecutionError` (`[ERR]`), never as a
    // lint-style `ViolationsFound` (`[FAIL]`). `biome check --write` has no
    // "found drift" exit code, so a non-zero exit is always operational.
    if !tooling::check_binary_exists("biome") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    fs::write(
      temp.path().join("broken.ts"),
      "const x: = = ;\nfunction (( {\n",
    )
    .unwrap();

    let surface = JavaScriptSurface;
    let mut ctx = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("javascript"),
    );
    ctx.check_only = true;

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on --check must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  #[test]
  fn javascript_write_reports_execution_error_on_formatter_failure() {
    // Fixes #155: the non-`--check` write path must classify the same
    // operational biome failure as `ExecutionError`, not `ViolationsFound`
    // — mirroring `javascript_check_reports_execution_error_on_formatter_failure`
    // above.
    if !tooling::check_binary_exists("biome") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    fs::write(
      temp.path().join("broken.ts"),
      "const x: = = ;\nfunction (( {\n",
    )
    .unwrap();

    let surface = JavaScriptSurface;
    let ctx = surfaces::test_ctx(
      temp.path(),
      config::ResolvedLangConfig::new("javascript"),
    );

    let res = surface.format(&ctx);
    assert!(
      matches!(res.status, surfaces::SurfaceStatus::ExecutionError { .. }),
      "a formatter failure on the write path must be ExecutionError, got: {:?}",
      res.status
    );
    assert!(!res.is_success());
  }

  /// Builds a context over a lone clean `.ts` file whose `extra_args` carry
  /// `extra`, for the `--linter-enabled` guard tests below.
  fn ctx_with_extra_args(
    temp: &tempfile::TempDir,
    extra: &[&str],
  ) -> surfaces::ExecutionContext {
    fs::write(temp.path().join("a.ts"), "const x = 1;\n").unwrap();
    let mut lang = config::ResolvedLangConfig::new("javascript");
    lang.extra_args = [(
      "biome".to_string(),
      extra.iter().map(|s| (*s).to_string()).collect(),
    )]
    .into();
    surfaces::test_ctx(temp.path(), lang)
  }

  /// Asserts `res` is the `--linter-enabled` override refusal, not some other
  /// `ExecutionError` (a real biome failure would also be `ExecutionError`,
  /// so matching on the variant alone would be a vacuous assertion).
  fn assert_linter_override_refusal(res: &surfaces::SurfaceResult) {
    match &res.status {
      surfaces::SurfaceStatus::ExecutionError { message } => {
        assert!(
          message.contains("--linter-enabled")
            && message.contains("extra_args")
            && message.contains("fml lint"),
          "diagnostic must name the flag, where it came from, and the way \
           out; got: {message}"
        );
      }
      other => panic!("expected ExecutionError, got {other:?}"),
    }
    assert!(!res.is_success());
  }

  #[test]
  fn javascript_format_refuses_extra_args_linter_enabled_override() {
    // Fixes #173: `--linter-enabled=true` in `extra_args` does *not* re-enable
    // biome's linter — the format path passes the flag itself, and biome
    // rejects the duplicate outright (verified against the pinned
    // `@biomejs/biome@2.5.10`). What the user got before was an accurate but
    // opaque `[ERR]` naming neither `extra_args` nor the flag; the surface now
    // refuses up front with a diagnostic that explains the cause. Hermetic:
    // the guard runs before `tool_missing_guard`, so no biome install is
    // needed. Asserted on both the `--check` and write branches, which pass
    // the flag alike.
    let temp = tempfile::TempDir::new().unwrap();
    let ctx = ctx_with_extra_args(&temp, &["--linter-enabled=true"]);
    assert_linter_override_refusal(&JavaScriptSurface.format(&ctx));

    let mut check_ctx = ctx_with_extra_args(&temp, &["--linter-enabled=true"]);
    check_ctx.check_only = true;
    assert_linter_override_refusal(&JavaScriptSurface.format(&check_ctx));
  }

  #[test]
  fn javascript_format_refuses_redundant_linter_enabled_restatement() {
    // Fixes #173: `--linter-enabled=false` looks like a harmless restatement
    // of what `fml fmt` already passes, but biome rejects the duplicate flag
    // outright ("argument `--linter-enabled` cannot be used multiple times in
    // this context") before it parses the value — so this spelling was just
    // as broken as `=true`, and gets the same explanation rather than being
    // let through. Hermetic, per the guard's placement above the tool guard.
    let temp = tempfile::TempDir::new().unwrap();
    let ctx = ctx_with_extra_args(&temp, &["--linter-enabled=false"]);
    assert_linter_override_refusal(&JavaScriptSurface.format(&ctx));
  }

  #[test]
  fn javascript_format_allows_unrelated_extra_args() {
    // The #173 guard is narrow: only the one flag `fml` passes itself is
    // refused. An unrelated `extra_args` entry must still format normally, or
    // the guard would be a regression for every other user.
    if !tooling::check_binary_exists("biome") {
      return;
    }
    let temp = tempfile::TempDir::new().unwrap();
    let ctx = ctx_with_extra_args(&temp, &["--no-errors-on-unmatched"]);
    let res = JavaScriptSurface.format(&ctx);
    assert!(
      res.is_success(),
      "an unrelated extra_args entry must still format, got: {:?}",
      res.status
    );
  }

  #[test]
  fn linter_enabled_override_message_quotes_the_offending_argument() {
    // Hermetic (no biome needed): the diagnostic echoes the argument as the
    // user wrote it, so it is greppable in their own `formality.toml`.
    let message = linter_enabled_override_message("--linter-enabled true");
    assert!(
      message.contains("`--linter-enabled true`"),
      "got: {message}"
    );
    assert!(message.contains("--linter-enabled=false"), "got: {message}");
  }
}
