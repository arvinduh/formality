//! YAML language surface: formats via Prettier and lints via yamllint.
//!
//! Implements `super::LanguageSurface` for YAML, syncing `.yamllint.yaml`.
//! Fleet registration is owned by `super::registry`.

use std::path;
use std::time;

use crate::config;
use crate::config::facets;
use crate::surfaces;
use crate::surfaces::sync;
use crate::surfaces::sync::native;
use crate::surfaces::sync::prettier;
use crate::surfaces::tooling;

/// The `.yamllint.yaml` settings `ctx` resolves to. yamllint's `-d` takes
/// the same document inline, so `fml lint` passes [`native::ToolConfig::yaml`].
fn yamllint_config(ctx: &surfaces::ExecutionContext) -> native::ToolConfig {
  let yaml = ctx.lang_config.yaml.as_ref();
  let toggle = |on: Option<bool>| {
    if on == Some(true) {
      "enable"
    } else {
      "disable"
    }
  };
  native::ToolConfig::new(".yamllint.yaml")
    .set("extends", "default")
    .set(
      "rules.line-length.max",
      native::int(ctx.lang_config.line_length),
    )
    .set(
      "rules.indentation.spaces",
      native::int(ctx.lang_config.indent_size),
    )
    .set(
      "rules.indentation.indent-sequences",
      yaml.and_then(|y| y.indent_sequence).unwrap_or(true),
    )
    .set(
      "rules.document-start",
      toggle(yaml.and_then(|y| y.document_start)),
    )
    .set("rules.truthy", toggle(yaml.and_then(|y| y.truthy)))
}

/// Builds the argument vector for `yamllint -f parsable <file>`, used by
/// `fml lsp`'s structured-diagnostics path (Fixes #165 [pre-recreation]). `parsable` is
/// yamllint's long-stable gcc-style line format —
/// `path:line:col: [level] message (rule)` — verified against a locally
/// installed yamllint. Like the existing clippy/ruff diagnostics paths, this
/// intentionally runs with yamllint's own default rule set rather than
/// threading through `yamllint_config`'s resolved
/// `formality.toml` settings — the same known simplification noted in this
/// module's callers (see `engine::lsp::diagnostics` module docs).
#[must_use]
pub fn build_yamllint_parsable_args(file: &path::Path) -> Vec<String> {
  vec![
    "-f".to_string(),
    "parsable".to_string(),
    file.to_string_lossy().to_string(),
  ]
}

/// YAML language surface implementation.
#[derive(Debug, Default)]
pub struct YamlSurface;

impl facets::DeclaresFacets for YamlSurface {
  fn facet_support(&self, facet: facets::Facet) -> facets::FacetSupport {
    match facet {
      facets::Facet::IndentTabs => facets::FacetSupport::Fixed("spaces"),
      facets::Facet::IndentWidth
      | facets::Facet::LineLength
      | facets::Facet::QuoteStyle
      | facets::Facet::ProseWrap => facets::FacetSupport::Configurable,
      facets::Facet::TrailingComma
      | facets::Facet::ImportSort
      | facets::Facet::Edition
      | facets::Facet::Standard => facets::FacetSupport::Unsupported,
    }
  }
}

const YAML_EXTENSIONS: &[&str] = &["yaml", "yml"];

impl surfaces::LanguageSurface for YamlSurface {
  fn name(&self) -> &'static str {
    "yaml"
  }

  fn extra_args_tools(&self) -> &'static [&'static str] {
    &["prettier", "yamllint"]
  }

  fn aliases(&self) -> &[&'static str] {
    &["yml"]
  }

  fn file_extensions(&self) -> &[&'static str] {
    YAML_EXTENSIONS
  }

  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(Self)
  }

  fn marker_files(&self) -> &[&'static str] {
    &[".yamllint", ".yamllint.yaml", ".yamllint.yml"]
  }

  fn tool_info(
    &self,
    _config: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![
      surfaces::ToolInfo {
        binary: "prettier",
        description: "YAML formatter",
        install_hint: None,
        is_required_for_fmt: true,
        is_required_for_lint: false,
      },
      surfaces::ToolInfo {
        binary: "yamllint",
        description: "YAML linter",
        install_hint: None,
        is_required_for_fmt: false,
        is_required_for_lint: true,
      },
    ]
  }

  fn format(
    &self,
    ctx: &surfaces::ExecutionContext,
  ) -> surfaces::SurfaceResult {
    let start = time::Instant::now();

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "prettier", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(YAML_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `--tab-width`/`--print-width`/etc. instead of writing
    // `.prettierrc.json` to disk — see `prettier::prettier_args` (Fixes
    // #151 [pre-recreation]). `fml sync` remains the only path that materializes the file.
    let inline_config = prettier::prettier_args(ctx);

    if ctx.check_only {
      return sync::diff_check_via_tempcopy_classified(
        &files,
        |scratch| {
          let mut cmd = tooling::create_tool_command("prettier");
          cmd
            .arg("--write")
            .arg("--parser")
            .arg("yaml")
            .args(&inline_config)
            .arg(scratch);
          cmd.args(ctx.lang_config.tool_args("prettier"));
          cmd.current_dir(ctx.root.as_path());
          cmd.output()
        },
        self.name(),
        start,
        tooling::classify_all_nonzero_as_error,
      );
    }

    let mut cmd = tooling::create_tool_command("prettier");
    cmd.arg("--write");
    cmd.args(&inline_config);

    for f in &files {
      cmd.arg(f);
    }

    cmd.args(ctx.lang_config.tool_args("prettier"));
    cmd.current_dir(ctx.root.as_path());

    // `prettier --write` exits `0` whether or not it reformats and only
    // exits non-zero (`2`) on a parse error / bad config / unreadable file
    // — never `1`, which is `--check`-only. So every non-zero exit here is
    // a tool failure (`ExecutionError`), not formatting drift, and the
    // `--check` path above classifies identically (Fixes #107).
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

    if fix {
      return tooling::lint_fix_unsupported(self.name(), start);
    }

    if let Some(res) =
      tooling::tool_missing_guard(self.name(), "yamllint", start, None)
    {
      return res;
    }

    let files = ctx.matched_files(YAML_EXTENSIONS);
    if let Some(res) = surfaces::passed_if_empty(&files, self.name(), start) {
      return res;
    }

    // Inline `-d <yaml source>` instead of writing `.yamllint.yaml` to disk
    // — see `yamllint_config` (Fixes #151 [pre-recreation]). `fml sync` remains
    // the only path that materializes the file.
    let inline_config = yamllint_config(ctx).yaml();

    let mut cmd = tooling::create_tool_command("yamllint");
    cmd.arg("-d").arg(&inline_config);
    if !ctx.paths.is_empty()
      || !ctx.lang_config.files.is_empty()
      || !ctx.lang_config.exclude.is_empty()
    {
      for f in &files {
        cmd.arg(f);
      }
    } else {
      cmd.arg(".");
    }

    cmd.args(ctx.lang_config.tool_args("yamllint"));
    cmd.current_dir(ctx.root.as_path());

    tooling::run_tool_command(self.name(), &mut cmd)
  }

  fn uses_prettier(&self) -> bool {
    true
  }

  // `fml fmt`/`fml lint` no longer go through this path (Fixes #151 [pre-recreation]): they
  // pass the resolved config to prettier/yamllint inline (see
  // `prettier::prettier_args` and `yamllint_config`, used in
  // `format()`/`lint()` above). This method is now reached only by `fml
  // sync`, for users who explicitly want `.yamllint.yaml` materialized on
  // disk (Fixes #158 [pre-recreation]: previously this never called
  // `yamllint_config(ctx).sync`, so `.yamllint.yaml` was never
  // actually written by `fml sync`).
  //
  // `.prettierrc.json` is deliberately not written here — it is shared with
  // the JSON and Markdown surfaces and has exactly one writer, the shared
  // pass `sync_shared_prettier_config` (#130). `uses_prettier` above is this
  // surface's declaration that it consumes that file.
  fn sync_config(
    &self,
    ctx: &surfaces::ExecutionContext,
    check: bool,
  ) -> surfaces::SurfaceResult {
    yamllint_config(ctx).sync(ctx, check, time::Instant::now(), self.name())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::surfaces::LanguageSurface;

  /// Rules default to disabled and sequence indentation on; set options flip
  /// them. The inline `-d` document carries the same rules, minus the header.
  #[test]
  fn yamllint_config_table() {
    let temp = tempfile::TempDir::new().unwrap();
    let flipped = config::options::YamlOptions {
      indent_sequence: Some(false),
      document_start: Some(true),
      truthy: Some(true),
    };
    let cases = [
      (
        None,
        [
          "document-start: disable",
          "truthy: disable",
          "indent-sequences: true",
        ],
      ),
      (
        Some(flipped),
        [
          "document-start: enable",
          "truthy: enable",
          "indent-sequences: false",
        ],
      ),
    ];
    for (yaml, expected) in cases {
      let mut lang = config::ResolvedLangConfig::new("yaml");
      lang.yaml = yaml;
      lang.line_length = 120;
      let cfg = yamllint_config(&surfaces::test_ctx(temp.path(), lang));
      let (file, inline) = (cfg.render(), cfg.yaml());
      assert!(file.starts_with(native::AUTO_GENERATED_HEADER), "{file}");
      assert!(
        !inline.starts_with(native::AUTO_GENERATED_HEADER),
        "{inline}"
      );
      for line in expected.into_iter().chain(["max: 120"]) {
        assert!(file.contains(line), "{line} missing from file:\n{file}");
        assert!(inline.contains(line), "{line} missing inline:\n{inline}");
      }
    }
  }

  #[test]
  fn test_build_yamllint_parsable_args() {
    let args = build_yamllint_parsable_args(path::Path::new("config/app.yaml"));
    assert_eq!(
      args,
      vec![
        "-f".to_string(),
        "parsable".to_string(),
        "config/app.yaml".to_string(),
      ]
    );
  }

  #[test]
  fn yaml_sync_config_writes_yamllint_config() {
    let temp = tempfile::TempDir::new().unwrap();
    let surface = YamlSurface;
    let mut lang_cfg = config::ResolvedLangConfig::new("yaml");
    lang_cfg.line_length = 100;
    lang_cfg.indent_size = 4;
    lang_cfg.yaml = Some(config::options::YamlOptions {
      indent_sequence: Some(false),
      document_start: Some(true),
      truthy: Some(true),
    });

    let ctx = surfaces::test_ctx(temp.path(), lang_cfg);

    let res = surface.sync_config(&ctx, false);
    // Fixes #130: the file this surface writes is named in its result, and
    // the shared `.prettierrc.json` is not written from inside the fan-out.
    assert_eq!(res.status.created_file_names(), [".yamllint.yaml"]);
    assert!(surface.uses_prettier());
    assert!(!temp.path().join(".prettierrc.json").exists());

    // Fixes #158 [pre-recreation]: `fml sync` must also materialize `.yamllint.yaml`.
    let yamllint_path = temp.path().join(".yamllint.yaml");
    assert!(yamllint_path.is_file());
    let content = std::fs::read_to_string(&yamllint_path).unwrap();
    assert!(content.starts_with(native::AUTO_GENERATED_HEADER));
    assert!(content.contains("extends: default"));
    assert!(content.contains("max: 100"));
    assert!(content.contains("spaces: 4"));
    assert!(content.contains("indent-sequences: false"));
    assert!(content.contains("document-start: enable"));
    assert!(content.contains("truthy: enable"));
  }

  #[test]
  fn yaml_format_and_lint_do_not_write_native_files() {
    // Fixes #151 [pre-recreation]: `fml fmt`/`fml lint` must not write `.prettierrc.json` or
    // `.yamllint.yaml` as a side effect; only `fml sync` writes those files
    // (see `sync_config`, Fixes #158 [pre-recreation]).
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("a.yaml"), "a: 1\n").unwrap();

    let surface = YamlSurface;
    let ctx =
      surfaces::test_ctx(temp.path(), config::ResolvedLangConfig::new("yaml"));

    if tooling::check_binary_exists("prettier") {
      let _ = surface.format(&ctx);
    }
    if tooling::check_binary_exists("yamllint") {
      let _ = surface.lint(&ctx, false);
    }

    assert!(!temp.path().join(".prettierrc.json").exists());
    assert!(!temp.path().join(".yamllint.yaml").exists());
  }
}
