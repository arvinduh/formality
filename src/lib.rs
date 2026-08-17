pub mod cli;
pub mod config;
pub mod diff;
pub mod doctor;
pub mod facets;
pub mod lsp;
pub mod runner;
pub mod surfaces;
pub mod table;
pub mod update;
pub mod version;

use clap::Parser;
use cli::{Cli, Commands};
use colored::Colorize;
use config::{DEFAULT_CONFIG_FILE_NAME, FormalityConfig, find_project_config};
use runner::{Runner, RunnerAction};
use std::path::{Path, PathBuf};
use surfaces::{
  LanguageSurface, all_surfaces, detect_surfaces_smart, find_files_with_ext,
  get_surface_by_name,
};

/// The JSON Schema for formality.toml, embedded directly so the binary can
/// emit it via `fml schema` and the release pipeline can regenerate
/// `schema/formality.schema.json` without needing the file in the source tree.
pub const FORMALITY_SCHEMA_JSON: &str = r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "FormalityConfig",
  "description": "Configuration schema for formality (fml) - Multi-language formatting & linting orchestrator",
  "type": "object",
  "properties": {
    "global": {
      "type": "object",
      "description": "Shared universal formatting settings applied across all language surfaces unless overridden",
      "properties": {
        "languages": {
          "type": "array",
          "description": "Explicit allowlist of active languages. If specified, only these languages will be formatted or linted.",
          "items": {
            "type": "string",
            "enum": [
              "rust",
              "python",
              "cpp",
              "markdown",
              "yaml",
              "json",
              "toml",
              "typst"
            ]
          }
        },
        "ignore_languages": {
          "type": "array",
          "description": "Languages to completely ignore and exclude from formatting, linting, and config syncing.",
          "items": {
            "type": "string",
            "enum": [
              "rust",
              "python",
              "cpp",
              "markdown",
              "yaml",
              "json",
              "toml",
              "typst"
            ]
          }
        },
        "indent_size": {
          "type": "integer",
          "minimum": 1,
          "default": 2,
          "description": "Default indentation width in spaces"
        },
        "line_length": {
          "type": "integer",
          "minimum": 20,
          "default": 80,
          "description": "Default maximum line length / column limit"
        },
        "end_of_line": {
          "type": "string",
          "enum": ["lf", "crlf", "cr", "auto"],
          "default": "lf",
          "description": "Line termination style"
        },
        "charset": {
          "type": "string",
          "default": "utf-8",
          "description": "Character encoding"
        },
        "insert_final_newline": {
          "type": "boolean",
          "default": true,
          "description": "Ensure files end with a newline"
        },
        "trim_trailing_whitespace": {
          "type": "boolean",
          "default": true,
          "description": "Trim trailing whitespace on all lines"
        },
        "use_tabs": {
          "type": "boolean",
          "default": false,
          "description": "Use tabs instead of spaces for indentation"
        }
      },
      "additionalProperties": false
    },
    "lang": {
      "type": "object",
      "description": "Language-specific surfaces and overrides",
      "patternProperties": {
        "^(rust|python|cpp|markdown|yaml|json|toml|typst|[a-zA-Z0-9_-]+)$": {
          "type": "object",
          "properties": {
            "format_tool": {
              "type": "string",
              "description": "Formatter binary or tool name"
            },
            "lint_tool": {
              "type": "string",
              "description": "Linter binary or tool name"
            },
            "indent_size": {
              "type": "integer",
              "minimum": 1,
              "description": "Override global indent size for this language"
            },
            "line_length": {
              "type": "integer",
              "minimum": 20,
              "description": "Override global line length for this language"
            },
            "use_tabs": {
              "type": "boolean",
              "description": "Override tab indentation setting for this language"
            },
            "prose_wrap": {
              "type": "string",
              "enum": ["always", "never", "preserve"],
              "description": "Prose wrapping rule for Markdown"
            },
            "enabled": {
              "type": "boolean",
              "default": true,
              "description": "Enable or disable this surface"
            },
            "extra_args": {
              "type": "array",
              "items": { "type": "string" },
              "description": "Extra CLI flags passed down directly to the underlying tool"
            },
            "files": {
              "type": "array",
              "items": { "type": "string" },
              "description": "File patterns to include"
            },
            "exclude": {
              "type": "array",
              "items": { "type": "string" },
              "description": "File patterns to exclude"
            }
          },
          "additionalProperties": true
        }
      }
    }
  },
  "additionalProperties": false
}
"#;

pub fn run() -> i32 {
  let args = Cli::parse();
  run_with_args(args)
}

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

  match args.command {
    Commands::Schema { output } => {
      if let Some(target_file) = output {
        if let Some(parent) = target_file.parent() {
          let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&target_file, FORMALITY_SCHEMA_JSON) {
          Ok(_) => {
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
        println!("{}", FORMALITY_SCHEMA_JSON);
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
        Ok(_) => {
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

      println!(
        "{} {}",
        "fml surfaces".bold().cyan(),
        format!("({} supported)", all_surfaces().len()).dimmed()
      );
      println!(
        "{}",
        "──────────────────────────────────────────────────────────────────"
          .dimmed()
      );

      let mut active_count = 0;
      for surface in all_surfaces() {
        let is_detected = detected_names.contains(&surface.name());
        let (marker, name_colored) = if is_detected {
          active_count += 1;
          ("[ACTIVE]  ".green().bold(), surface.name().bold())
        } else {
          ("[INACTIVE]".dimmed(), surface.name().dimmed())
        };

        let aliases_str = if !surface.aliases().is_empty() {
          format!("aliases: {}", surface.aliases().join(", "))
        } else {
          String::new()
        };

        println!("  {} {:<14} {}", marker, name_colored, aliases_str.dimmed());
      }
      println!(
        "{}",
        "──────────────────────────────────────────────────────────────────"
          .dimmed()
      );
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

      if install {
        doctor::preflight_install(&surfaces, &config, true);
      }

      Runner::run(
        surfaces,
        &root,
        &target_paths,
        RunnerAction::Format { check },
        &config,
      )
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

      if install {
        doctor::preflight_install(&surfaces, &config, false);
      }

      Runner::run(
        surfaces,
        &root,
        &target_paths,
        RunnerAction::Lint { fix },
        &config,
      )
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
          print!("{}", rendered);
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

pub fn get_git_staged_files(root: &Path) -> Result<Vec<PathBuf>, String> {
  let output = std::process::Command::new("git")
    .arg("diff")
    .arg("--name-only")
    .arg("--cached")
    .arg("--diff-filter=ACMR")
    .current_dir(root)
    .output()
    .map_err(|e| format!("Failed to execute git: {}", e))?;

  if !output.status.success() {
    return Err("Failed to query git staged files.".to_string());
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let files: Vec<PathBuf> = stdout
    .lines()
    .map(|l| root.join(l.trim()))
    .filter(|p| p.is_file())
    .collect();

  Ok(files)
}

pub fn get_git_changed_files(root: &Path) -> Result<Vec<PathBuf>, String> {
  let output = std::process::Command::new("git")
    .arg("diff")
    .arg("--name-only")
    .arg("--diff-filter=ACMR")
    .current_dir(root)
    .output()
    .map_err(|e| format!("Failed to execute git: {}", e))?;

  if !output.status.success() {
    return Err("Failed to query git changed files.".to_string());
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let files: Vec<PathBuf> = stdout
    .lines()
    .map(|l| root.join(l.trim()))
    .filter(|p| p.is_file())
    .collect();

  Ok(files)
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
          "Unknown language surface: '{}'. Run 'fml list-surfaces' to see supported languages.",
          name
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
