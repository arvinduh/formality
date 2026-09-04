//! Shared Prettier configuration model, CLI argument builders, and configuration syncing.

use super::{
  AUTO_GENERATED_JSON_COMMENT, ExecutionContext, LanguageSurface, NativeConfig,
  SurfaceResult, SurfaceStatus, render_native_config, sync_file_helper,
  sync_native_config,
};
use crate::config::{
  FormalityConfig, ResolvedGlobalConfig, ResolvedLangConfig,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

/// Native `.prettierrc.json` configuration representation for Markdown, YAML, and JSON formatting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrettierConfig {
  /// Warning comment field.
  #[serde(rename = "$comment")]
  pub comment: String,
  /// Indentation tab width in spaces.
  pub tab_width: usize,
  /// Maximum print width limit.
  pub print_width: usize,
  /// Whether tab indentation is enabled.
  pub use_tabs: bool,
  /// End of line newline style.
  pub end_of_line: String,
  /// Prose wrapping strategy string.
  pub prose_wrap: String,
}

impl NativeConfig for PrettierConfig {
  const FILE_NAME: &'static str = ".prettierrc.json";

  fn from_context(ctx: &ExecutionContext) -> Self {
    Self::from_resolved(&ctx.global_config, &ctx.lang_config)
  }

  fn render(&self) -> Result<String, crate::errors::FormalityError> {
    render_native_config(self)
  }
}

impl PrettierConfig {
  /// Resolves the config from an already-resolved global/language pair.
  ///
  /// [`NativeConfig::from_context`] delegates here. The split exists because
  /// the shared single-writer pass ([`sync_shared_prettier_config`]) runs
  /// outside the per-surface fan-out and so has no [`ExecutionContext`] — it
  /// needs to resolve what *each* prettier surface would have asked for in
  /// order to detect a conflict between them.
  #[must_use]
  pub fn from_resolved(
    global: &ResolvedGlobalConfig,
    lang: &ResolvedLangConfig,
  ) -> Self {
    let eol = match global.end_of_line.to_lowercase().as_str() {
      "crlf" => "crlf",
      "cr" => "cr",
      _ => "lf",
    };

    Self {
      comment: AUTO_GENERATED_JSON_COMMENT.to_string(),
      tab_width: lang.indent_size,
      print_width: lang.line_length,
      use_tabs: lang.use_tabs,
      end_of_line: eol.to_string(),
      prose_wrap: lang.prose_wrap.as_deref().unwrap_or("always").to_string(),
    }
  }

  /// The settings this config carries, as `(key, value)` pairs in the order
  /// they are serialized. Used to explain a conflict between two surfaces
  /// that resolve `.prettierrc.json` differently.
  fn settings(&self) -> [(&'static str, String); 5] {
    [
      ("tabWidth", self.tab_width.to_string()),
      ("printWidth", self.print_width.to_string()),
      ("useTabs", self.use_tabs.to_string()),
      ("endOfLine", self.end_of_line.clone()),
      ("proseWrap", self.prose_wrap.clone()),
    ]
  }
}

/// Renders the resolved [`PrettierConfig`] as the inline `--tab-width`/
/// `--print-width`/etc. flags `prettier` accepts on the CLI, so `fml fmt`
/// can apply formality.toml's settings without writing `.prettierrc.json`
/// to disk (Fixes #151). Shared by the Markdown, YAML, and JSON surfaces,
/// which all format via prettier. Only `fml sync` writes that file now.
#[must_use]
pub fn build_prettier_inline_args(cfg: &PrettierConfig) -> Vec<String> {
  let mut args = vec![
    format!("--tab-width={}", cfg.tab_width),
    format!("--print-width={}", cfg.print_width),
    format!("--end-of-line={}", cfg.end_of_line),
    format!("--prose-wrap={}", cfg.prose_wrap),
  ];
  if cfg.use_tabs {
    args.push("--use-tabs".to_string());
  }
  args
}

/// Synchronizes `.prettierrc.json` for a single surface.
///
/// **Not reachable from `fml sync` any more** — see
/// [`sync_shared_prettier_config`], which owns the file. Retained because a
/// surface may still want to materialize the file on its own (and because
/// `sync_native_config::<PrettierConfig>` is the natural spelling for it),
/// but calling this from more than one surface in the same run reintroduces
/// the concurrent-write bug this module's shared pass exists to prevent.
#[must_use]
pub fn sync_prettier_config(
  ctx: &ExecutionContext,
  check: bool,
  start: Instant,
  surface_name: &'static str,
) -> SurfaceResult {
  sync_native_config::<PrettierConfig>(ctx, check, start, surface_name)
}

/// Surface name reported by the shared `.prettierrc.json` pass, mirroring
/// how the shared `.editorconfig` pass reports itself as `editorconfig`.
pub const PRETTIER_PASS_NAME: &str = "prettier";

/// Synchronizes the one root `.prettierrc.json` on behalf of **every**
/// prettier-formatted surface in the run, exactly once.
///
/// # Why this exists
///
/// `json`, `markdown` and `yaml` all format via prettier, and all three used
/// to call `sync_prettier_config` from their own `sync_config` — which the
/// runner invokes concurrently under `surfaces.par_iter()`. Three threads
/// therefore ran the read-compare-write in [`sync_file_helper`] against the
/// same path with no coordination (#130). Consequences, in ascending
/// severity:
///
/// - Whichever thread won reported `Created .prettierrc.json` and the others
///   reported `Passed`, so *which surface got the credit* varied run to run.
/// - `fml sync --check` could disagree with itself the same way, and it is a
///   `.pre-commit-hooks.yaml` entry point.
/// - On Windows a second `fs::write` while the first still holds the handle
///   can fail with a sharing violation — a rare, unreproducible spurious
///   error on an otherwise fine run.
///
/// # The fix, and why coalescing rather than locking
///
/// The write is removed from the fan-out entirely rather than serialized
/// inside it. A mutex would make the writes safe but leave the report
/// nondeterministic — the surface named in the output would still be
/// whichever thread got the lock first — and would leave three writers for
/// one file, which is the wrong shape regardless. Hoisting the write beside
/// the existing shared `.editorconfig` pass gives the file exactly one
/// writer, one row, and a stable name (`prettier`), so repeated runs produce
/// byte-identical output.
///
/// # Conflicting settings are an error, not a coin flip
///
/// [`PrettierConfig::from_context`] resolves from each surface's *own*
/// `[lang.<name>]` block, so `[lang.markdown] line_length = 100` beside a
/// global `80` genuinely asks for two different `.prettierrc.json` files.
/// With the old fan-out that was last-writer-wins, silently. Since one path
/// cannot hold both, this reports an explicit conflict naming the surfaces
/// and the settings they disagree on, and writes nothing — silence is what
/// made the original bug invisible.
///
/// Returns `None` when no surface in the run formats via prettier, so no row
/// is rendered for a `.prettierrc.json` nobody asked for.
#[must_use]
pub fn sync_shared_prettier_config(
  root: &Path,
  config: &FormalityConfig,
  surfaces: &[Box<dyn LanguageSurface>],
  check: bool,
) -> Option<SurfaceResult> {
  let start = Instant::now();
  let global = config.resolve_global();

  // Deterministic order: the surfaces are matched in a fixed order, so the
  // conflict message and the "winning" config are stable run to run.
  let claims: Vec<(&'static str, PrettierConfig)> = surfaces
    .iter()
    .filter(|s| s.uses_prettier())
    .map(|s| {
      let lang = config.resolve_for_lang_with_global(s.name(), &global);
      (s.name(), PrettierConfig::from_resolved(&global, &lang))
    })
    .collect();

  let (_, expected) = claims.first()?;

  if let Some(message) = describe_prettier_conflict(&claims) {
    return Some(SurfaceResult {
      surface_name: PRETTIER_PASS_NAME,
      status: SurfaceStatus::ExecutionError { message },
      duration: start.elapsed(),
    });
  }

  let content = match expected.render() {
    Ok(c) => c,
    Err(e) => {
      return Some(SurfaceResult {
        surface_name: PRETTIER_PASS_NAME,
        status: SurfaceStatus::ExecutionError {
          message: format!(
            "Failed to serialize {}: {e}",
            PrettierConfig::FILE_NAME
          ),
        },
        duration: start.elapsed(),
      });
    }
  };

  Some(sync_file_helper(
    &root.join(PrettierConfig::FILE_NAME),
    PrettierConfig::FILE_NAME,
    &content,
    check,
    start,
    PRETTIER_PASS_NAME,
  ))
}

/// Explains a disagreement between two prettier surfaces about the single
/// shared `.prettierrc.json`, or `None` when they all agree.
///
/// The first claim is the reference: every other surface is compared against
/// it, and only the settings that actually differ are listed, so the message
/// points at the `[lang.<name>]` override the user needs to change rather
/// than dumping both configs.
fn describe_prettier_conflict(
  claims: &[(&'static str, PrettierConfig)],
) -> Option<String> {
  let (first_name, first_cfg) = claims.first()?;
  let conflicting: Vec<&(&'static str, PrettierConfig)> = claims
    .iter()
    .skip(1)
    .filter(|(_, cfg)| cfg != first_cfg)
    .collect();
  if conflicting.is_empty() {
    return None;
  }

  let file = PrettierConfig::FILE_NAME;
  let mut msg = format!(
    "'{file}' is a single file shared by every prettier-formatted surface, \
     but these surfaces resolve it to conflicting settings:\n"
  );
  for (name, cfg) in conflicting {
    for ((key, mine), (_, theirs)) in
      cfg.settings().iter().zip(first_cfg.settings())
    {
      if *mine != theirs {
        let _ = writeln!(
          msg,
          "  {key}: {name} wants {mine}, {first_name} wants {theirs}"
        );
      }
    }
  }
  let _ = write!(
    msg,
    "\nNothing was written — one path cannot hold both. Align the \
     conflicting '[lang.<name>]' overrides in formality.toml (or move the \
     setting to the global table) so every prettier surface agrees.\n\
     \n\
     Note that 'fml fmt' is unaffected: it passes each surface's own \
     settings to prettier inline and never reads '{file}'. This file exists \
     for editors and other tools that read it directly."
  );
  Some(msg)
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn test_prettier_config_typed_serialization() {
    let cfg = PrettierConfig {
      comment: "warning".to_string(),
      tab_width: 4,
      print_width: 100,
      use_tabs: true,
      end_of_line: "crlf".to_string(),
      prose_wrap: "preserve".to_string(),
    };
    let rendered = cfg.render().unwrap();
    assert!(rendered.contains("\"$comment\": \"warning\""));
    assert!(rendered.contains("\"tabWidth\": 4"));
    assert!(rendered.contains("\"printWidth\": 100"));
    assert!(rendered.contains("\"useTabs\": true"));
    assert!(rendered.contains("\"endOfLine\": \"crlf\""));
    assert!(rendered.contains("\"proseWrap\": \"preserve\""));
  }

  use crate::surfaces::{json, markdown, rust, yaml};
  use std::path::Path;

  fn prettier_surfaces() -> Vec<Box<dyn LanguageSurface>> {
    vec![
      Box::new(json::JsonSurface),
      Box::new(markdown::MarkdownSurface),
      Box::new(yaml::YamlSurface),
    ]
  }

  #[test]
  fn test_only_prettier_surfaces_declare_the_shared_config() {
    // The declaration, not a hardcoded name list, is what keeps the shared
    // pass in step with the surfaces (#130).
    assert!(json::JsonSurface.uses_prettier());
    assert!(markdown::MarkdownSurface.uses_prettier());
    assert!(yaml::YamlSurface.uses_prettier());
    assert!(!rust::RustSurface.uses_prettier());
  }

  #[test]
  fn test_shared_pass_writes_prettierrc_once_for_three_surfaces() {
    // Fixes #130: json, markdown and yaml all claimed `.prettierrc.json` and
    // wrote it concurrently under `surfaces.par_iter()`. The write is now
    // coalesced into one pass outside the fan-out, so there is exactly one
    // writer, one row, and a stable surface name in the report.
    let temp = tempfile::TempDir::new().unwrap();
    let config = FormalityConfig::default();

    let res = sync_shared_prettier_config(
      temp.path(),
      &config,
      &prettier_surfaces(),
      false,
    )
    .expect("a run containing prettier surfaces must sync the file");

    assert_eq!(res.surface_name, PRETTIER_PASS_NAME);
    assert_eq!(res.status.created_file_names(), [".prettierrc.json"]);
    assert!(temp.path().join(".prettierrc.json").is_file());

    // Re-running is a no-op, and reports as one rather than as a second
    // creation — which is what makes repeated `fml sync` output identical.
    let again = sync_shared_prettier_config(
      temp.path(),
      &config,
      &prettier_surfaces(),
      false,
    )
    .expect("still syncing");
    assert!(matches!(again.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_shared_pass_is_absent_when_no_surface_uses_prettier() {
    let temp = tempfile::TempDir::new().unwrap();
    let surfaces: Vec<Box<dyn LanguageSurface>> =
      vec![Box::new(rust::RustSurface)];
    assert!(
      sync_shared_prettier_config(
        temp.path(),
        &FormalityConfig::default(),
        &surfaces,
        false
      )
      .is_none(),
      "no row for a .prettierrc.json nobody asked for"
    );
    assert!(!temp.path().join(".prettierrc.json").exists());
  }

  #[test]
  fn test_conflicting_lang_overrides_are_an_explicit_error_not_a_coin_flip() {
    // One path cannot hold two configurations. Under the old fan-out this
    // was last-writer-wins between three racing threads; it is now a loud,
    // deterministic error that names both surfaces and the setting they
    // disagree on, and nothing is written.
    let toml_str = "
      line_length = 80
      [lang.markdown]
      line_length = 100
    ";
    let config =
      FormalityConfig::parse_str(toml_str, Path::new("formality.toml"))
        .unwrap();
    let temp = tempfile::TempDir::new().unwrap();

    let res = sync_shared_prettier_config(
      temp.path(),
      &config,
      &prettier_surfaces(),
      false,
    )
    .expect("prettier surfaces are present");

    let SurfaceStatus::ExecutionError { message } = &res.status else {
      panic!("expected an explicit conflict, got {:?}", res.status);
    };
    assert!(message.contains("printWidth"), "{message}");
    assert!(message.contains("markdown"), "{message}");
    assert!(message.contains("json"), "{message}");
    assert!(
      !temp.path().join(".prettierrc.json").exists(),
      "a conflicting config must not be written"
    );
  }

  #[test]
  fn test_agreeing_surfaces_report_no_conflict() {
    let global = ResolvedGlobalConfig::default();
    let config = FormalityConfig::default();
    let claims: Vec<(&'static str, PrettierConfig)> = ["json", "markdown"]
      .into_iter()
      .map(|n| {
        let lang = config.resolve_for_lang_with_global(n, &global);
        (n, PrettierConfig::from_resolved(&global, &lang))
      })
      .collect();
    assert!(describe_prettier_conflict(&claims).is_none());
  }

  #[test]
  fn test_from_resolved_matches_from_context() {
    // `from_context` delegates to `from_resolved`; the shared pass depends
    // on the two agreeing, since it resolves without an ExecutionContext.
    let config = FormalityConfig::default();
    let global = config.resolve_global();
    let lang = config.resolve_for_lang_with_global("markdown", &global);
    let temp = tempfile::TempDir::new().unwrap();
    let mut ctx = crate::surfaces::test_ctx(temp.path(), lang.clone());
    ctx.global_config = std::sync::Arc::new(global.clone());

    assert_eq!(
      PrettierConfig::from_context(&ctx),
      PrettierConfig::from_resolved(&global, &lang)
    );
  }
}
