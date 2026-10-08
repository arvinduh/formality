//! Shared Prettier configuration generator and CLI argument builder.
//!
//! Models Prettier options and command-line arguments. Surfaces driving Prettier
//! include `super::markdown`, `super::json`, and `super::yaml`.

use std::fmt::Write;
use std::path;
use std::time;

use crate::config;
use crate::surfaces;
use crate::surfaces::LanguageSurface;
use crate::surfaces::sync::native;

/// The shared `.prettierrc.json` file name.
pub const FILE_NAME: &str = ".prettierrc.json";

/// The `.prettierrc.json` settings a global/language pair resolves to.
///
/// Takes resolved configs rather than an execution context because the
/// shared single-writer pass ([`sync_shared_prettier_config`]) runs outside
/// the per-surface fan-out and compares what each surface would ask for.
#[must_use]
pub fn prettier_config(
  global: &config::ResolvedGlobalConfig,
  lang: &config::ResolvedLangConfig,
) -> native::ToolConfig {
  let eol = match global.end_of_line.to_lowercase().as_str() {
    "crlf" => "crlf",
    "cr" => "cr",
    _ => "lf",
  };
  native::ToolConfig::new(FILE_NAME)
    .set("$comment", native::AUTO_GENERATED_JSON_COMMENT)
    .set("tabWidth", native::int(lang.indent_size))
    .set("printWidth", native::int(lang.line_length))
    .set("useTabs", lang.use_tabs)
    .set("endOfLine", eol)
    .set("proseWrap", lang.prose_wrap.as_deref().unwrap_or("always"))
}

/// The prettier flags `fml fmt` passes inline instead of writing
/// `.prettierrc.json`; shared by the Markdown, YAML and JSON surfaces.
#[must_use]
pub fn prettier_args(ctx: &surfaces::ExecutionContext) -> Vec<String> {
  prettier_config(&ctx.global_config, &ctx.lang_config).flags(&[
    ("tab-width", "tabWidth"),
    ("print-width", "printWidth"),
    ("end-of-line", "endOfLine"),
    ("prose-wrap", "proseWrap"),
    ("use-tabs", "useTabs"),
  ])
}

/// Surface name reported by the shared `.prettierrc.json` pass, mirroring
/// how the shared `.editorconfig` pass reports itself as `editorconfig`.
const PRETTIER_PASS_NAME: &str = "prettier";

/// Synchronizes the one root `.prettierrc.json` on behalf of **every**
/// prettier-formatted surface in the run, exactly once.
///
/// # Why this exists
///
/// `json`, `markdown` and `yaml` all format via prettier, and all three used
/// to sync it from their own
/// `sync_config` — which the runner invokes concurrently under
/// `surfaces.par_iter()`. Three threads
/// therefore ran the read-compare-write in `sync_file_helper` against the
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
/// [`prettier_config`] resolves from each surface's *own*
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
  root: &path::Path,
  config: &config::FormalityConfig,
  surfaces: &[Box<dyn LanguageSurface>],
  check: bool,
) -> Option<surfaces::SurfaceResult> {
  let start = time::Instant::now();
  let global = config.resolve_global();

  // Deterministic order: the surfaces are matched in a fixed order, so the
  // conflict message and the "winning" config are stable run to run.
  let claims: Vec<(&'static str, native::ToolConfig)> = surfaces
    .iter()
    .filter(|s| s.uses_prettier())
    .map(|s| {
      let lang = config.resolve_for_lang_with_global(s.name(), &global);
      (s.name(), prettier_config(&global, &lang))
    })
    .collect();

  let (_, expected) = claims.first()?;

  if let Some(message) = describe_prettier_conflict(&claims) {
    return Some(surfaces::SurfaceResult::error(
      PRETTIER_PASS_NAME,
      start,
      message,
    ));
  }

  Some(super::sync_file_helper(
    &root.join(FILE_NAME),
    FILE_NAME,
    &expected.render(),
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
  claims: &[(&'static str, native::ToolConfig)],
) -> Option<String> {
  let (first_name, first_cfg) = claims.first()?;
  let conflicting: Vec<&(&'static str, native::ToolConfig)> = claims
    .iter()
    .skip(1)
    .filter(|(_, cfg)| cfg != first_cfg)
    .collect();
  if conflicting.is_empty() {
    return None;
  }

  let file = FILE_NAME;
  let mut msg = format!(
    "'{file}' is a single file shared by every prettier-formatted surface, \
     but these surfaces resolve it to conflicting settings:\n"
  );
  for (name, cfg) in conflicting {
    for ((key, mine), (_, theirs)) in cfg.entries().zip(first_cfg.entries()) {
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
mod tests {
  use std::sync;

  use super::*;

  /// The file and the inline flags carry the same values; `useTabs` is a bare
  /// flag that only appears when true.
  #[test]
  fn prettier_file_and_flags_agree() {
    let temp = tempfile::TempDir::new().unwrap();
    for use_tabs in [false, true] {
      let mut lang = config::ResolvedLangConfig::new("markdown");
      lang.indent_size = 4;
      lang.line_length = 100;
      lang.use_tabs = use_tabs;
      lang.prose_wrap = Some("preserve".to_string());
      let mut ctx = surfaces::test_ctx(temp.path(), lang);
      ctx.global_config = sync::Arc::new(config::ResolvedGlobalConfig {
        end_of_line: "crlf".to_string(),
        ..Default::default()
      });
      let file = prettier_config(&ctx.global_config, &ctx.lang_config).render();
      for line in [
        "\"tabWidth\": 4",
        "\"printWidth\": 100",
        "\"endOfLine\": \"crlf\"",
        "\"proseWrap\": \"preserve\"",
      ] {
        assert!(file.contains(line), "{line} missing:\n{file}");
      }
      assert!(file.contains("\"$comment\""), "{file}");
      let mut expected = vec![
        "--tab-width=4",
        "--print-width=100",
        "--end-of-line=crlf",
        "--prose-wrap=preserve",
      ];
      if use_tabs {
        expected.push("--use-tabs");
      }
      assert_eq!(prettier_args(&ctx), expected);
    }
  }

  fn prettier_surfaces() -> Vec<Box<dyn LanguageSurface>> {
    vec![
      Box::new(surfaces::lang::json::JsonSurface),
      Box::new(surfaces::lang::markdown::MarkdownSurface),
      Box::new(surfaces::lang::yaml::YamlSurface),
    ]
  }

  #[test]
  fn only_prettier_surfaces_declare_the_shared_config() {
    // The declaration, not a hardcoded name list, is what keeps the shared
    // pass in step with the surfaces (#130).
    assert!(surfaces::lang::json::JsonSurface.uses_prettier());
    assert!(surfaces::lang::markdown::MarkdownSurface.uses_prettier());
    assert!(surfaces::lang::yaml::YamlSurface.uses_prettier());
    assert!(!surfaces::lang::rust::RustSurface.uses_prettier());
  }

  #[test]
  fn shared_pass_writes_prettierrc_once_for_three_surfaces() {
    // Fixes #130: json, markdown and yaml all claimed `.prettierrc.json` and
    // wrote it concurrently under `surfaces.par_iter()`. The write is now
    // coalesced into one pass outside the fan-out, so there is exactly one
    // writer, one row, and a stable surface name in the report.
    let temp = tempfile::TempDir::new().unwrap();
    let config = config::FormalityConfig::default();

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
    assert!(matches!(again.status, surfaces::SurfaceStatus::Passed));
  }

  #[test]
  fn shared_pass_is_absent_when_no_surface_uses_prettier() {
    let temp = tempfile::TempDir::new().unwrap();
    let surfaces: Vec<Box<dyn LanguageSurface>> =
      vec![Box::new(surfaces::lang::rust::RustSurface)];
    assert!(
      sync_shared_prettier_config(
        temp.path(),
        &config::FormalityConfig::default(),
        &surfaces,
        false
      )
      .is_none(),
      "no row for a .prettierrc.json nobody asked for"
    );
    assert!(!temp.path().join(".prettierrc.json").exists());
  }

  #[test]
  fn conflicting_lang_overrides_are_an_explicit_error_not_a_coin_flip() {
    // One path cannot hold two configurations. Under the old fan-out this
    // was last-writer-wins between three racing threads; it is now a loud,
    // deterministic error that names both surfaces and the setting they
    // disagree on, and nothing is written.
    let toml_str = "
      [global]
      line_length = 80
      [lang.markdown]
      line_length = 100
    ";
    let config = config::FormalityConfig::parse_str(
      toml_str,
      path::Path::new("formality.toml"),
    )
    .unwrap();
    let temp = tempfile::TempDir::new().unwrap();

    let res = sync_shared_prettier_config(
      temp.path(),
      &config,
      &prettier_surfaces(),
      false,
    )
    .expect("prettier surfaces are present");

    let surfaces::SurfaceStatus::ExecutionError { message } = &res.status
    else {
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
  fn agreeing_surfaces_report_no_conflict() {
    let global = config::ResolvedGlobalConfig::default();
    let config = config::FormalityConfig::default();
    let claims: Vec<(&'static str, native::ToolConfig)> = ["json", "markdown"]
      .into_iter()
      .map(|n| {
        let lang = config.resolve_for_lang_with_global(n, &global);
        (n, prettier_config(&global, &lang))
      })
      .collect();
    assert!(describe_prettier_conflict(&claims).is_none());
  }
}
