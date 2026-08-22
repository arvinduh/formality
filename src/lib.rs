pub mod cli;
pub mod commands;
pub mod config;
pub mod editorconfig;
pub mod engine;
pub mod surfaces;
pub mod ui;

// Backward-compatible top-level module aliases so existing `crate::foo::*`
// and `fml::foo::*` paths (integration tests, external consumers) keep
// working after the domain-driven `src/` reorganization.
//
// These aliases are a compatibility shim only: internal code in this crate
// always spells out the canonical, structural path (e.g.
// `crate::ui::table`, `crate::engine::version`) rather than the shortened
// alias, even where the alias would resolve to the same item. Keeping that
// distinction consistent means the alias list can eventually be trimmed or
// deprecated without touching any internal call site.
pub use commands::doctor;
pub use commands::lsp;
pub use config::facets;
pub use config::schema;
pub use engine::diff;
pub use engine::runner;
pub use engine::update;
pub use engine::version;
pub use ui::table;

use clap::Parser;
use cli::{Cli, Commands};
use colored::Colorize;
use config::{DEFAULT_CONFIG_FILE_NAME, FormalityConfig, find_project_config};
pub use editorconfig::generate_editorconfig;
use engine::{Runner, RunnerAction};
use std::path::{Path, PathBuf};
use surfaces::{
  LanguageSurface, all_surfaces, detect_surfaces_smart, find_files_with_ext,
  get_surface_by_name,
};

pub use schema::generate_schema;

#[must_use]
pub fn run() -> i32 {
  let args = Cli::parse();
  run_with_args(args)
}

#[must_use]
pub fn run_with_args(args: Cli) -> i32 {
  if std::env::var("FORCE_COLOR").is_ok()
    || std::env::var("CLICOLOR_FORCE").is_ok()
    || std::env::var("GITHUB_ACTIONS").is_ok()
  {
    colored::control::set_override(true);
  }

  let update_notifier = update::spawn_update_check();
  let code = run_command_inner(args);
  update::print_update_notice(update_notifier);
  code
}

// Dispatches all top-level CLI commands (fmt, lint, sync, fix, doctor, init, lsp, schema, etc.).
#[allow(clippy::too_many_lines)]
fn run_command_inner(args: Cli) -> i32 {
  let root = args.root.unwrap_or_else(|| {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
  });

  let (mut config, _config_path) =
    match FormalityConfig::load_layered(Some(&root)) {
      Ok(res) => res,
      Err(e) => {
        eprintln!("{} {}", "Config error:".red().bold(), e);
        return 2;
      }
    };

  if let Some(custom_cfg) = args.config {
    match FormalityConfig::load_file(&custom_cfg) {
      Ok(custom) => config.merge(custom),
      Err(e) => {
        eprintln!("{} {}", "Custom config error:".red().bold(), e);
        return 2;
      }
    }
  }

  warn_unrecognized_lang_sections(&config);

  match args.command {
    Commands::Schema { output } => {
      let schema_json = generate_schema();
      if let Some(target_file) = output {
        if let Some(parent) = target_file.parent() {
          let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&target_file, &schema_json) {
          Ok(()) => {
            println!(
              "{} Wrote JSON Schema to {}",
              "[OK]".green().bold(),
              target_file.display().to_string().cyan()
            );
            0
          }
          Err(e) => {
            eprintln!(
              "{} Failed to write schema to {}: {}",
              "[ERR]".red().bold(),
              target_file.display(),
              e
            );
            2
          }
        }
      } else {
        println!("{schema_json}");
        0
      }
    }

    Commands::Doctor { all, install } => {
      doctor::run_doctor(&root, all, install, &config)
    }

    Commands::Install { all } => doctor::run_doctor(&root, all, true, &config),

    Commands::Init { force, hidden } => {
      let target_file_name = if hidden {
        ".formality.toml"
      } else {
        DEFAULT_CONFIG_FILE_NAME
      };
      let target = root.join(target_file_name);

      if let Some(existing) = find_project_config(&root) {
        if !force {
          eprintln!(
            "{} Config file already exists at {}. Use {} to overwrite.",
            "[ERR]".red().bold(),
            existing.display(),
            "--force".bold()
          );
          return 1;
        }
        // Warn when --force would create a file that is shadowed by an existing
        // higher-priority config (e.g. creating .formality.toml while
        // formality.toml already exists).
        if existing != target && existing.exists() {
          eprintln!(
            "{} '{}' already exists and takes precedence over '{}'. \
             The new file will be shadowed and ignored unless '{}' is removed.",
            "[WARN]".yellow().bold(),
            existing.display(),
            target_file_name,
            existing.display(),
          );
        }
      }

      let detected = detect_surfaces_smart(&root, &config);
      let detected_names: Vec<&str> =
        detected.iter().map(|s| s.name()).collect();
      let template = FormalityConfig::generate_init_template(&detected_names);

      match std::fs::write(&target, template) {
        Ok(()) => {
          println!(
            "{} Initialized {} with {} detected surface(s).",
            "[OK]".green().bold(),
            target.display().to_string().cyan(),
            detected.len()
          );
          0
        }
        Err(e) => {
          eprintln!(
            "{} Failed to write {}: {}",
            "[ERR]".red().bold(),
            target.display(),
            e
          );
          2
        }
      }
    }

    Commands::ListSurfaces => {
      let detected = detect_surfaces_smart(&root, &config);
      let detected_names: Vec<&str> =
        detected.iter().map(|s| s.name()).collect();

      let mut surfaces_table = crate::ui::table::Table::new(vec![
        crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
          .width(crate::ui::table::WidthPolicy::Fixed(12)),
        crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
          .width(crate::ui::table::WidthPolicy::Fixed(14)),
        crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
          .width(crate::ui::table::WidthPolicy::Auto),
      ])
      .layout(crate::ui::table::Layout::compact().indent(2).padding(0, 1));

      let mut active_count = 0;
      for surface in all_surfaces() {
        let is_detected = detected_names.contains(&surface.name());
        let (status_style, name_style, marker) = if is_detected {
          active_count += 1;
          (
            crate::ui::table::Style::Ok,
            crate::ui::table::Style::Strong,
            "[ACTIVE]  ",
          )
        } else {
          (
            crate::ui::table::Style::Dim,
            crate::ui::table::Style::Dim,
            "[INACTIVE]",
          )
        };

        let aliases_str = if surface.aliases().is_empty() {
          String::new()
        } else {
          format!("aliases: {}", surface.aliases().join(", "))
        };

        surfaces_table.add_row(crate::ui::table::Row::new(vec![
          crate::ui::table::Cell::styled(marker, status_style),
          crate::ui::table::Cell::styled(surface.name(), name_style),
          crate::ui::table::Cell::styled(
            aliases_str,
            crate::ui::table::Style::Dim,
          ),
        ]));
      }

      let palette = crate::ui::table::Palette::detect();
      let rendered_table = crate::ui::table::render(&surfaces_table, &palette);
      let separator = crate::ui::table::separator_for_content(&rendered_table);

      println!(
        "{} {}",
        "fml surfaces".bold().cyan(),
        format!("({} supported)", all_surfaces().len()).dimmed()
      );
      println!("{}", separator.dimmed());
      if !rendered_table.is_empty() {
        println!("{rendered_table}");
      }
      println!("{}", separator.dimmed());
      println!(
        "  {} active, {} inactive\n",
        active_count.to_string().green().bold(),
        (all_surfaces().len() - active_count).to_string().dimmed()
      );
      0
    }

    Commands::Fmt {
      check,
      staged,
      changed,
      lang,
      install,
      paths,
    } => {
      let target_paths = match resolve_git_paths(&root, staged, changed, paths)
      {
        Ok(p) => p,
        Err(e) => {
          eprintln!("{}", e.red().bold());
          return 2;
        }
      };

      let surfaces =
        match resolve_target_surfaces(&root, &lang, &target_paths, &config) {
          Ok(s) => s,
          Err(e) => {
            eprintln!("{}", e.red().bold());
            return 2;
          }
        };

      let mut install_failed = false;
      if install && !doctor::preflight_install(&surfaces, &config, true) {
        warn_tool_install_failed("formatting");
        install_failed = true;
      }

      let code = Runner::run(
        surfaces,
        &root,
        &target_paths,
        RunnerAction::Format { check },
        &config,
      );
      if install_failed && code == 0 { 2 } else { code }
    }

    Commands::Fix {
      staged,
      changed,
      lang,
      install,
      paths,
    } => {
      let target_paths = match resolve_git_paths(&root, staged, changed, paths)
      {
        Ok(p) => p,
        Err(e) => {
          eprintln!("{}", e.red().bold());
          return 2;
        }
      };

      let surfaces =
        match resolve_target_surfaces(&root, &lang, &target_paths, &config) {
          Ok(s) => s,
          Err(e) => {
            eprintln!("{}", e.red().bold());
            return 2;
          }
        };

      let mut install_failed = false;
      if install {
        let lint_ok = doctor::preflight_install(&surfaces, &config, false);
        let fmt_ok = doctor::preflight_install(&surfaces, &config, true);
        if !lint_ok || !fmt_ok {
          warn_tool_install_failed("fixes");
          install_failed = true;
        }
      }

      let code =
        Runner::run(surfaces, &root, &target_paths, RunnerAction::Fix, &config);
      if install_failed && code == 0 { 2 } else { code }
    }

    Commands::Lint {
      fix,
      staged,
      changed,
      lang,
      install,
      paths,
    } => {
      let target_paths = match resolve_git_paths(&root, staged, changed, paths)
      {
        Ok(p) => p,
        Err(e) => {
          eprintln!("{}", e.red().bold());
          return 2;
        }
      };

      let surfaces =
        match resolve_target_surfaces(&root, &lang, &target_paths, &config) {
          Ok(s) => s,
          Err(e) => {
            eprintln!("{}", e.red().bold());
            return 2;
          }
        };

      let mut install_failed = false;
      if install && !doctor::preflight_install(&surfaces, &config, false) {
        warn_tool_install_failed("linting");
        install_failed = true;
      }

      let code = Runner::run(
        surfaces,
        &root,
        &target_paths,
        RunnerAction::Lint { fix },
        &config,
      );
      if install_failed && code == 0 { 2 } else { code }
    }

    Commands::Sync { check, lang } => {
      let surfaces = match resolve_target_surfaces(&root, &lang, &[], &config) {
        Ok(s) => s,
        Err(e) => {
          eprintln!("{}", e.red().bold());
          return 2;
        }
      };
      Runner::run(surfaces, &root, &[], RunnerAction::Sync { check }, &config)
    }

    Commands::Lsp => {
      lsp::run_lsp_server(Some(root));
      0
    }

    Commands::Table { json } => {
      let json_str = if let Some(j) = json {
        j
      } else {
        use std::io::Read;
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
          eprintln!(
            "{} Failed to read table spec from stdin: {}",
            "[ERR]".red().bold(),
            e
          );
          return 2;
        }
        buf
      };
      match table::render_json(&json_str) {
        Ok(rendered) => {
          print!("{rendered}");
          0
        }
        Err(e) => {
          eprintln!("{} Invalid table JSON spec: {}", "[ERR]".red().bold(), e);
          2
        }
      }
    }
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

fn warn_tool_install_failed(verb: &str) {
  eprintln!(
    "{} One or more required tools failed to install automatically; {verb} may be skipped for affected languages.",
    "[WARN]".yellow().bold()
  );
}

fn resolve_git_paths(
  root: &Path,
  staged: bool,
  changed: bool,
  explicit_paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
  if staged && changed {
    return Err(
      "--staged and --changed are mutually exclusive. Use one or the other."
        .to_string(),
    );
  }
  if staged {
    return get_git_staged_files(root);
  }
  if changed {
    return get_git_changed_files(root);
  }
  Ok(explicit_paths)
}

fn get_git_diff_files(
  root: &Path,
  staged: bool,
  error_context: &str,
) -> Result<Vec<PathBuf>, String> {
  let mut cmd = std::process::Command::new("git");
  cmd.arg("diff").arg("--name-only");
  if staged {
    cmd.arg("--cached");
  }
  cmd.arg("--diff-filter=ACMR").current_dir(root);

  let output = cmd
    .output()
    .map_err(|e| format!("Failed to execute git: {e}"))?;

  if !output.status.success() {
    return Err(format!("Failed to query git {error_context} files."));
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let files: Vec<PathBuf> = stdout
    .lines()
    .map(|l| root.join(l.trim()))
    .filter(|p| p.is_file())
    .collect();

  Ok(files)
}

/// Returns the list of staged git files relative to `root`.
///
/// # Errors
///
/// Returns an error message if git execution fails or the git command cannot be run.
pub fn get_git_staged_files(root: &Path) -> Result<Vec<PathBuf>, String> {
  get_git_diff_files(root, true, "staged")
}

/// Returns the list of changed git files relative to `root`.
///
/// # Errors
///
/// Returns an error message if git execution fails or the git command cannot be run.
pub fn get_git_changed_files(root: &Path) -> Result<Vec<PathBuf>, String> {
  get_git_diff_files(root, false, "changed")
}

fn resolve_target_surfaces(
  root: &Path,
  lang_filter: &[String],
  paths: &[PathBuf],
  config: &FormalityConfig,
) -> Result<Vec<Box<dyn LanguageSurface>>, String> {
  if !lang_filter.is_empty() {
    let mut selected = Vec::new();
    for name in lang_filter {
      if let Some(s) = get_surface_by_name(name) {
        selected.push(s);
      } else {
        return Err(format!(
          "Unknown language surface: '{name}'. Run 'fml list-surfaces' to see supported languages."
        ));
      }
    }
    return Ok(selected);
  }

  if !paths.is_empty() {
    let mut active = Vec::new();
    for surface in all_surfaces() {
      let lang_cfg = config.resolve_for_lang(surface.name());
      let matching = find_files_with_ext(
        root,
        surface.file_extensions(),
        paths,
        &lang_cfg.files,
        &lang_cfg.exclude,
      );
      if !matching.is_empty() {
        active.push(surface);
      }
    }
    return Ok(active);
  }

  Ok(detect_surfaces_smart(root, config))
}
