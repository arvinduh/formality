//! Formality (`fml`) is a unified CLI for formatting, linting, and syncing configurations across multiple language surfaces.

#![warn(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]

/// Command-line argument parsing definitions.
pub mod cli;
/// CLI command implementations.
pub mod commands;
/// Configuration loading, parsing, and resolving.
pub mod config;
/// Execution engine for running formatters, linters, and version checks.
pub mod engine;
/// Crate-wide error types and exit status handling.
pub mod errors;
/// Language surface definitions and registry.
pub mod surfaces;
/// Terminal UI components and layout rendering.
pub mod ui;

// `generate_schema` and `SCHEMA_VERSION` are re-exported at the crate root
// because this crate's own integration tests (`tests/schema_drift.rs`,
// `tests/integration_tests.rs`) reach them as `fml::generate_schema` /
// `fml::SCHEMA_VERSION` — a real external use, not a compatibility shim.
// Every other item in this crate is reached through its canonical,
// structural module path (e.g. `crate::engine::update`,
// `crate::ui::table`); see docs/style-guide.md §1.
pub use config::SCHEMA_VERSION;
pub use config::schema::generate_schema;

use cli::{Cli, Commands, MigrateCommands};
use colored::Colorize;
use config::FormalityConfig;
use errors::{ExitStatus, FormalityError};
use std::path::{Path, PathBuf};

/// Parses CLI arguments from `std::env::args()` and executes the command.
#[must_use]
pub fn run() -> ExitStatus {
  let args = Cli::parse_checked();
  run_with_args(args)
}

/// Executes the CLI command specified by the provided [`Cli`] arguments.
#[must_use]
pub fn run_with_args(args: Cli) -> ExitStatus {
  // NO_COLOR wins over every force-color signal, matching the precedence
  // `ui::table::Palette::detect` already applies to this crate's own escape
  // codes. Without the first branch the two disagreed under CI, where
  // GITHUB_ACTIONS/CLICOLOR_FORCE are set: the palette went plain while
  // `colored` was still forced on, so `NO_COLOR=1 fml ...` emitted a log
  // that was *mostly* uncolored but still carried bold/color runs around
  // every status token -- honoring neither mode, and defeating the point
  // of NO_COLOR for anything parsing the output.
  if crate::ui::no_color_requested() {
    colored::control::set_override(false);
  } else if crate::ui::color_forced() {
    colored::control::set_override(true);
  }

  let root = args.root.clone().unwrap_or_else(|| {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
  });

  let project_config_path = config::find_project_config(&root);

  let update_notifier = engine::update::spawn_update_check();
  let schema_notifier =
    config::schema::spawn_schema_check(project_config_path.as_deref());
  let status = run_command_inner(args, &root, project_config_path.as_deref());
  config::schema::print_schema_notice(schema_notifier);
  engine::update::print_update_notice(update_notifier);
  status
}

// Dispatches all top-level CLI commands (fmt, lint, sync, fix, doctor, init, lsp, schema, etc.).
fn run_command_inner(
  args: Cli,
  root: &Path,
  project_config_path: Option<&Path>,
) -> ExitStatus {
  let (mut config, _config_path) =
    match FormalityConfig::load_layered_with_path(project_config_path) {
      Ok(res) => res,
      Err(e) => {
        FormalityError::from(e).print_diagnostic();
        return ExitStatus::Error;
      }
    };

  if let Some(custom_cfg) = args.config {
    match FormalityConfig::load_file(&custom_cfg) {
      Ok(custom) => config.merge(custom),
      Err(e) => {
        FormalityError::from(e).print_diagnostic();
        return ExitStatus::Error;
      }
    }
  }

  warn_unrecognized_lang_sections(&config);

  match args.command {
    Commands::Schema { output } => commands::schema::run_schema(output),

    Commands::Doctor { all, install } => {
      commands::doctor::run_doctor(root, all, install, &config)
    }

    Commands::Install { all } => {
      commands::doctor::run_doctor(root, all, true, &config)
    }

    Commands::Init { force, hidden } => {
      commands::init::run_init(root, &config, force, hidden)
    }

    Commands::ListSurfaces => commands::surfaces::run_surfaces(root, &config),

    Commands::Fmt {
      check,
      staged,
      changed,
      lang,
      install,
      paths,
    } => commands::fmt::run_fmt(
      root, &config, check, staged, changed, lang, install, paths,
    ),

    Commands::Fix {
      check,
      staged,
      changed,
      lang,
      install,
      paths,
    } => commands::fix::run_fix(
      root, &config, check, staged, changed, lang, install, paths,
    ),

    // `--fix` is the deprecated spelling of `fml fix` and dispatches to it
    // outright, rather than to a lint-only writing form. That form no
    // longer exists: a lint-fix pass without the format pass that follows
    // it leaves the tree lint-fixed but unformatted, which is exactly the
    // state `.agents/orchestrate.md` §5 says `fml` must never leave
    // behind — and it was the sole source of the `fml fix` /
    // `fml lint --fix` ambiguity. The notice says so, and the run banner
    // reads `fml fix`, because that is genuinely what runs.
    Commands::Lint {
      fix: true,
      staged,
      changed,
      lang,
      install,
      paths,
      ..
    } => {
      crate::ui::deprecation::warn_deprecated_spelling(
        "fml lint --fix",
        "fml fix",
        Some(
          "it applies the same lint fixes and then reformats, which `fml lint --fix` never did",
        ),
      );
      commands::fix::run_fix(
        root, &config, false, staged, changed, lang, install, paths,
      )
    }

    Commands::Lint {
      staged,
      changed,
      lang,
      install,
      paths,
      ..
    } => commands::lint::run_lint(
      root, &config, staged, changed, lang, install, paths,
    ),

    Commands::Sync { check, lang } => {
      commands::sync::run_sync(root, &config, check, lang)
    }

    Commands::Lsp => {
      commands::lsp::run_lsp_server(Some(root.to_path_buf()));
      ExitStatus::Clean
    }

    Commands::Table { json } => commands::table::run_table(json),

    Commands::Migrate { command } => match command {
      MigrateCommands::Schema => commands::migrate::run_migrate_schema(root),
    },
  }
}

/// Warns (non-fatal, to stderr) about any `[lang.X]` sections in the
/// resolved config whose `X` isn't a recognized surface name or alias —
/// almost always a typo (e.g. `[lang.pythonn]`) that would otherwise be
/// silently ignored, leaving the user's override never applied and no
/// signal as to why. Runs once at config-load time so every subcommand
/// benefits, mirroring the `Unknown language surface` error already given
/// for an unrecognized `--lang` CLI flag value.
///
/// Deliberately does not flag a section that names a real surface which
/// simply isn't detected/active in the current workspace (e.g.
/// `[lang.rust]` in a Python-only repo) — that's a valid
/// pre-configuration, not a mistake.
fn warn_unrecognized_lang_sections(config: &FormalityConfig) {
  let registry = surfaces::SurfaceRegistry::default();
  let unrecognized = config.unrecognized_lang_sections(&registry);
  if unrecognized.is_empty() {
    return;
  }

  for name in unrecognized {
    eprintln!(
      "{} Unrecognized language section '[lang.{}]' in formality.toml — \
       this override will not be applied. Run '{}' to see supported \
       languages.",
      "[WARN]".yellow().bold(),
      name.bold(),
      "fml list-surfaces".cyan()
    );
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  // Tier-2 enforcement for the module/file hierarchy rule documented in
  // docs/style-guide.md ("`*_tests.rs` vs `#[cfg(test)] mod tests`"): a
  // `#[test]` walking the filesystem, same mechanism `registry.rs`'s fleet
  // side-table checks established (#113) — reused here, not reinvented.
  //
  // The rule: test modules live inline (`#[cfg(test)] mod tests { ... }`) in
  // the file under test. The one sanctioned exception is a directory module
  // (`some/mod.rs`) large enough that its tests live in a sibling `tests.rs`
  // declared via `mod tests;` — never any other `*_tests.rs` name. #120
  // deliberately collapsed every previous `<name>_tests.rs` file back inline;
  // this test keeps that convention from silently drifting back.
  #[test]
  fn test_no_stray_test_files_outside_sanctioned_pattern() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();
    for entry in ignore::WalkBuilder::new(&src_dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    {
      let path = entry.path();
      let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        continue;
      };
      if file_name == "tests.rs" {
        // Sanctioned only as a sibling of a directory module's `mod.rs`.
        let has_sibling_mod_rs = path.with_file_name("mod.rs").is_file();
        if !has_sibling_mod_rs {
          violations.push(format!(
            "{}: `tests.rs` with no sibling `mod.rs` — inline the tests in \
             the module file instead",
            path.display()
          ));
        }
      } else if file_name.ends_with("_tests.rs") {
        violations.push(format!(
          "{}: `*_tests.rs` naming is not the sanctioned pattern — inline \
           `#[cfg(test)] mod tests {{ ... }}` in the module file, or (only \
           for a directory module) use a sibling file named exactly \
           `tests.rs`",
          path.display()
        ));
      }
    }

    assert!(
      violations.is_empty(),
      "module/file hierarchy violation(s) — see docs/style-guide.md §1:\n{}",
      violations.join("\n")
    );
  }

  // Tier-2 enforcement for the naming-conventions predicate-method rule
  // documented in docs/style-guide.md §2 ("a pure getter or predicate ...
  // carries #[must_use]"), promoted from tier 3 during #133's sweep.
  //
  // Normalizes the signature before matching: strips a leading `pub` /
  // `pub(crate)` / `pub(super)` / `pub(in ...)` visibility modifier and any
  // `const`/`async`/`unsafe` qualifiers (in any order), then requires the
  // remainder to start with `fn is_`. Signatures are joined across lines up
  // to the opening `{` (or a trailing `;` for a trait-method declaration)
  // before checking for `-> bool`, so a return type on its own line is not
  // invisible to the scan. This was proven against a real gap in an earlier
  // version of this test: it matched only single-line `pub fn is_...(...)
  // -> bool` signatures, which passed green even with `#[must_use]` deleted
  // from `ExitStatus::is_clean` (a `pub const fn`) — i.e. it didn't catch
  // the rule's own named exemplar. See docs/style-guide.md §2.
  #[test]
  fn test_is_predicate_methods_carry_must_use() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    // Strips a leading `pub`/`pub(...)` visibility modifier and any
    // `const`/`async`/`unsafe` qualifiers, then reports whether what's left
    // starts a `fn is_*` predicate signature.
    fn starts_is_predicate_fn(trimmed: &str) -> bool {
      let mut rest = trimmed;

      if let Some(after_pub) = rest.strip_prefix("pub") {
        let after_pub = after_pub.trim_start();
        rest = if let Some(after_paren_open) = after_pub.strip_prefix('(') {
          match after_paren_open.find(')') {
            Some(close) => after_paren_open[close + 1..].trim_start(),
            None => after_pub,
          }
        } else {
          after_pub
        };
      }

      loop {
        let mut advanced = false;
        for kw in ["const", "async", "unsafe"] {
          if let Some(after_kw) = rest.strip_prefix(kw)
            && after_kw.starts_with(char::is_whitespace)
          {
            rest = after_kw.trim_start();
            advanced = true;
          }
        }
        if !advanced {
          break;
        }
      }

      rest.starts_with("fn is_")
    }

    let mut violations = Vec::new();
    for entry in ignore::WalkBuilder::new(&src_dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
      .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
    {
      let path = entry.path();
      let Ok(content) = std::fs::read_to_string(path) else {
        continue;
      };
      let lines: Vec<&str> = content.lines().collect();

      for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !starts_is_predicate_fn(trimmed) {
          continue;
        }

        // Join the signature across lines (a wrapped multi-line
        // `fn is_foo(\n  ...\n) -> bool {` is common in this crate) up to
        // the opening `{`, or a trailing `;` for a trait-method
        // declaration with no body, whichever comes first.
        let mut header = String::new();
        let mut is_bool_predicate = false;
        for l in lines[i..].iter().take(20) {
          if let Some(pos) = l.find('{') {
            header.push_str(&l[..pos]);
            is_bool_predicate = header.contains("-> bool");
            break;
          }
          let trimmed_end = l.trim_end();
          if let Some(without_semi) = trimmed_end.strip_suffix(';') {
            header.push_str(without_semi);
            is_bool_predicate = header.contains("-> bool");
            break;
          }
          header.push_str(l);
          header.push(' ');
        }
        if !is_bool_predicate {
          continue;
        }

        // Walk upward past any attributes/doc comments other than
        // `#[must_use]` to find whether one is present immediately above
        // the signature (allowing for other attributes in between, e.g.
        // `#[must_use]` then a doc comment is not how this crate writes
        // it, but tolerate ordering rather than over-fitting the scan).
        let has_must_use = lines[..i]
          .iter()
          .rev()
          .take_while(|prior| {
            let t = prior.trim_start();
            t.starts_with('#') || t.starts_with("///") || t.starts_with("//!")
          })
          .any(|prior| prior.trim_start().starts_with("#[must_use]"));

        if !has_must_use {
          violations.push(format!(
            "{}:{}: `{}` is missing `#[must_use]` — see docs/style-guide.md §2",
            path.display(),
            i + 1,
            trimmed
          ));
        }
      }
    }

    assert!(
      violations.is_empty(),
      "predicate-method `#[must_use]` violation(s) — see docs/style-guide.md §2:\n{}",
      violations.join("\n")
    );
  }

  // Tier-2 enforcement for the `//!` module-doc rule documented in
  // docs/style-guide.md §3 ("Every file with meaningful crate-level content
  // ... opens with a `//!` module-level doc comment"), promoted from tier 3
  // during #201's QA follow-up: a QA review of #201 found the rule was
  // ~80% unmet across the tree (41 of 50 files at the time) despite the PR
  // claiming a clean style-guide sweep, precisely because nothing mechanical
  // was checking it. Exempts `tests.rs` sibling files (the §1 directory-
  // module test-split exception) the same way an inline `#[cfg(test)] mod
  // tests` block is exempt — both are test-only content carrying the
  // `#[allow(missing_docs, ...)]` attribute from §3's second bullet, not
  // "meaningful crate-level content" in the production sense.
  #[test]
  fn test_files_carry_module_doc_comment() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();
    for entry in ignore::WalkBuilder::new(&src_dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
      .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
    {
      let path = entry.path();
      if path.file_name().and_then(|n| n.to_str()) == Some("tests.rs") {
        continue;
      }
      let Ok(content) = std::fs::read_to_string(path) else {
        continue;
      };
      let has_module_doc = content
        .lines()
        .take(15)
        .any(|l| l.trim_start().starts_with("//!"));
      if !has_module_doc {
        violations.push(format!(
          "{}: missing a `//!` module-level doc comment — see docs/style-guide.md §3",
          path.display()
        ));
      }
    }

    assert!(
      violations.is_empty(),
      "module-doc violation(s) — see docs/style-guide.md §3:\n{}",
      violations.join("\n")
    );
  }

  // Tier-2 enforcement for the `pub mod` doc comment rule documented in
  // docs/style-guide.md §3 ("Every `pub mod` declaration ... carries an outer
  // `///` doc comment one line above the `mod` keyword describing what the
  // module is for").
  #[test]
  fn test_pub_mod_declarations_carry_doc_comments() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();
    for entry in ignore::WalkBuilder::new(&src_dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
      .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
    {
      let path = entry.path();
      let Ok(content) = std::fs::read_to_string(path) else {
        continue;
      };
      let lines: Vec<&str> = content.lines().collect();

      for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        // Check for `pub mod <name>;` or `pub(...) mod <name>;`
        let is_pub_mod = if let Some(after_pub) = trimmed.strip_prefix("pub") {
          let rest = after_pub.trim_start();
          let rest = if let Some(after_paren) = rest.strip_prefix('(') {
            match after_paren.find(')') {
              Some(close) => after_paren[close + 1..].trim_start(),
              None => rest,
            }
          } else {
            rest
          };
          rest.starts_with("mod ") && rest.ends_with(';')
        } else {
          false
        };

        if !is_pub_mod {
          continue;
        }

        // Check if there is an outer doc comment `///` above it
        let has_doc_comment = lines[..i]
          .iter()
          .rev()
          .take_while(|prior| {
            let t = prior.trim_start();
            t.starts_with('#') || t.starts_with("///") || t.starts_with("//!")
          })
          .any(|prior| prior.trim_start().starts_with("///"));

        if !has_doc_comment {
          violations.push(format!(
            "{}:{}: `{}` is missing an outer `///` doc comment — see docs/style-guide.md §3",
            path.display(),
            i + 1,
            trimmed
          ));
        }
      }
    }

    assert!(
      violations.is_empty(),
      "`pub mod` doc comment violation(s) — see docs/style-guide.md §3:\n{}",
      violations.join("\n")
    );
  }

  // Tier-2 enforcement for the test-module allow-doc-lints rule documented in
  // docs/style-guide.md §3 ("An inline `#[cfg(test)] mod tests` block, or a
  // directory module's sibling `mod tests;` declaration ... carries
  // `#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]`
  // directly under the `#[cfg(test)]` attribute").
  #[test]
  fn test_test_modules_carry_allow_doc_lints() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();
    for entry in ignore::WalkBuilder::new(&src_dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
      .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
    {
      let path = entry.path();
      let Ok(content) = std::fs::read_to_string(path) else {
        continue;
      };
      let lines: Vec<&str> = content.lines().collect();

      for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let is_mod_tests = trimmed.starts_with("mod tests {")
          || trimmed.starts_with("mod tests;")
          || (trimmed.starts_with("pub mod tests") && trimmed.ends_with(';'));

        if !is_mod_tests {
          continue;
        }

        // Check attributes immediately above `mod tests`
        let attrs: Vec<&str> = lines[..i]
          .iter()
          .rev()
          .take_while(|prior| {
            let t = prior.trim_start();
            t.starts_with('#') || t.starts_with("///") || t.starts_with("//")
          })
          .map(|l| l.trim_start())
          .collect();

        let has_cfg_test = attrs.iter().any(|a| a.starts_with("#[cfg(test)]"));
        if !has_cfg_test {
          // If it's not a #[cfg(test)] module, skip
          continue;
        }

        let has_allow_missing_docs = attrs.iter().any(|a| {
          a.contains("missing_docs")
            && a.contains("missing_errors_doc")
            && a.contains("missing_panics_doc")
        });

        if !has_allow_missing_docs {
          violations.push(format!(
            "{}:{}: `mod tests` is missing `#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]` — see docs/style-guide.md §3",
            path.display(),
            i + 1,
          ));
        }
      }
    }

    assert!(
      violations.is_empty(),
      "test module `#[allow(...)]` doc lints violation(s) — see docs/style-guide.md §3:\n{}",
      violations.join("\n")
    );
  }

  // Tier-2 enforcement for canonical module paths rule documented in
  // docs/style-guide.md §1 ("new internal code always spells out the canonical,
  // structural path (e.g. `crate::ui::table`, `crate::engine::version`) —
  // never a crate-root shortcut").
  #[test]
  fn test_internal_code_uses_canonical_module_paths() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");

    let mut violations = Vec::new();
    for entry in ignore::WalkBuilder::new(&src_dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
      .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
    {
      let path = entry.path();
      // src/lib.rs declares the root re-exports, so it's exempt from checking its own declarations
      if path == src_dir.join("lib.rs") {
        continue;
      }
      let Ok(content) = std::fs::read_to_string(path) else {
        continue;
      };
      for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
          continue;
        }
        // Disallow shortcuts like `crate::generate_schema` or `crate::SCHEMA_VERSION`
        if trimmed.contains("crate::generate_schema")
          || trimmed.contains("crate::SCHEMA_VERSION")
        {
          violations.push(format!(
            "{}:{}: uses crate-root re-export shortcut instead of canonical path (use `crate::config::schema::generate_schema` / `crate::config::SCHEMA_VERSION`) — see docs/style-guide.md §1",
            path.display(),
            i + 1
          ));
        }
      }
    }

    assert!(
      violations.is_empty(),
      "canonical module path violation(s) — see docs/style-guide.md §1:\n{}",
      violations.join("\n")
    );
  }
}
