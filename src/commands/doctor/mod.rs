pub mod gitignore;
pub mod venv;

pub use gitignore::{
  GitignoreHygieneIssue, GitignoreHygieneReport, check_gitignore_hygiene,
  check_gitignore_hygiene_content, is_pattern_ignored,
};
pub use venv::{
  VirtualEnvInfo, VirtualEnvSource, detect_virtualenv,
  detect_virtualenv_with_env, find_venv_interpreter,
};

use crate::config::FormalityConfig;
use crate::engine::version::{
  ToolStatus, Version, check_tool_compatibility, get_raw_tool_version,
  minimum_supported_tool_version, probe_tool_version,
};
use crate::surfaces::{
  LanguageSurface, ToolInfo, all_surfaces, create_tool_command,
  detect_surfaces_smart,
};
use crate::ui::table::{
  Cell, Column, Layout, Palette, Row, Span, Style, Table, WidthPolicy, render,
};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Install a deduplicated list of missing tools.
///
/// Prints progress to stdout and returns `true` if every tool either
/// installed successfully or already had a known auto-install command,
/// `false` if any tool could not be installed.
#[must_use]
pub fn install_missing_tools(missing: &[ToolInfo]) -> bool {
  if missing.is_empty() {
    return true;
  }

  let separator = crate::ui::table::separator_line(0);
  println!("\n{}", separator.dimmed());
  println!("{}", "Auto-installing Missing Toolchains:".bold().cyan());

  let mut all_ok = true;

  for tool in missing {
    if let Some((program, args)) = tool.get_auto_install_cmd() {
      println!(
        "\n  {} Installing {} via: {} {}",
        "[INSTALL]".cyan().bold(),
        tool.binary.bold(),
        program.cyan(),
        args.join(" ").cyan()
      );

      let mut cmd = create_tool_command(&program);
      cmd.args(&args);

      match cmd.status() {
        Ok(status) if status.success() => {
          println!(
            "    {} Successfully installed {}",
            "[OK]  ".green().bold(),
            tool.binary.bold()
          );
        }
        Ok(status) => {
          println!(
            "    {} Failed to install {} (exit code: {})",
            "[FAIL]".red().bold(),
            tool.binary.bold(),
            status.code().unwrap_or(1)
          );
          all_ok = false;
        }
        Err(e) => {
          println!(
            "    {} Error running {}: {}",
            "[ERR] ".red().bold(),
            program,
            e
          );
          all_ok = false;
        }
      }
    } else {
      println!(
        "\n  {} No automatic package manager found for {}.\n    Manual install: {}",
        "[MISS]".yellow().bold(),
        tool.binary.bold(),
        tool.install_hint
      );
      all_ok = false;
    }
  }

  all_ok
}

/// Collect the missing tools required by `surfaces` for the given action
/// (format or lint), then install them. Returns `false` if any tool
/// could not be installed.
#[must_use]
pub fn preflight_install(
  surfaces: &[Box<dyn LanguageSurface>],
  config: &FormalityConfig,
  for_fmt: bool,
) -> bool {
  use which::which;
  let mut seen: HashSet<&'static str> = HashSet::new();
  let mut missing: Vec<ToolInfo> = Vec::new();

  for surface in surfaces {
    let resolved = config.resolve_for_lang(surface.name());
    for tool in surface.tool_info(&resolved) {
      if seen.contains(tool.binary) {
        continue;
      }
      let needed = if for_fmt {
        tool.is_required_for_fmt
      } else {
        tool.is_required_for_lint
      };
      if needed && which(tool.binary).is_err() {
        seen.insert(tool.binary);
        missing.push(tool);
      }
    }
  }

  install_missing_tools(&missing)
}

pub struct ToolLookupResult {
  pub is_installed: bool,
  pub path: Option<String>,
  pub raw_version: Option<String>,
  pub parsed_version: Option<Version>,
  pub status: Option<ToolStatus>,
}
#[must_use]
pub fn run_doctor(
  root: &Path,
  show_all: bool,
  install: bool,
  config: &FormalityConfig,
) -> i32 {
  let surfaces: Vec<Box<dyn LanguageSurface>> = if show_all {
    all_surfaces()
  } else {
    let detected = detect_surfaces_smart(root, config);
    if detected.is_empty() {
      all_surfaces()
    } else {
      detected
    }
  };

  let (
    doctor_table,
    missing_unique_tools,
    installed_unique_tools,
    outdated_unique_tools,
  ) = scan_tools_and_build_table(&surfaces, config);

  let palette = Palette::detect();
  let rendered_table = render(&doctor_table, &palette);
  let separator = crate::ui::table::separator_for_content(&rendered_table);

  println!(
    "{} {}",
    "fml doctor".bold().cyan(),
    if show_all {
      "(all surfaces)".dimmed()
    } else {
      "(active surfaces)".dimmed()
    }
  );
  println!("{}", separator.dimmed());
  if !rendered_table.is_empty() {
    println!("{rendered_table}");
  }

  // Check for unconfigured surfaces if explicit `languages` is set
  print_unconfigured_languages(root, config, &separator);

  // Virtual Environment status
  print_virtualenv_status(root, &surfaces, show_all, &separator);

  // .gitignore Cache Hygiene Check
  print_gitignore_hygiene(root, &surfaces, &separator);

  // Auto-install mode
  let mut install_failed = false;
  if install
    && !missing_unique_tools.is_empty()
    && !install_missing_tools(&missing_unique_tools)
  {
    install_failed = true;
  }

  println!("{}", separator.dimmed());
  let outdated_str = if outdated_unique_tools.is_empty() {
    String::new()
  } else {
    format!(" ({} outdated)", outdated_unique_tools.len())
      .yellow()
      .to_string()
  };
  println!(
    "  {} installed{}, {} missing{}\n",
    installed_unique_tools.len().to_string().green().bold(),
    outdated_str,
    if missing_unique_tools.is_empty() {
      "0".green().bold().to_string()
    } else {
      missing_unique_tools
        .len()
        .to_string()
        .yellow()
        .bold()
        .to_string()
    },
    if !missing_unique_tools.is_empty() && !install {
      " (run 'fml install' to install missing tools)"
        .dimmed()
        .to_string()
    } else {
      String::new()
    }
  );

  if (missing_unique_tools.is_empty() || install) && !install_failed {
    0
  } else {
    2
  }
}

fn lookup_tool_info(binary: &'static str) -> ToolLookupResult {
  let is_installed = which::which(binary).is_ok()
    || (binary == "clippy"
      && (which::which("clippy-driver").is_ok()
        || which::which("cargo").is_ok()));

  if is_installed {
    let path = which::which(binary)
      .or_else(|_| which::which("clippy-driver"))
      .or_else(|_| which::which("cargo"))
      .ok()
      .map(|p| p.display().to_string());
    let raw_version = get_raw_tool_version(binary);
    let parsed_version = probe_tool_version(binary);
    let status = minimum_supported_tool_version(binary)
      .map(|mstv| check_tool_compatibility(binary, &mstv));

    ToolLookupResult {
      is_installed: true,
      path,
      raw_version,
      parsed_version,
      status,
    }
  } else {
    ToolLookupResult {
      is_installed: false,
      path: None,
      raw_version: None,
      parsed_version: None,
      status: Some(ToolStatus::NotFound),
    }
  }
}

fn scan_tools_and_build_table(
  surfaces: &[Box<dyn LanguageSurface>],
  config: &FormalityConfig,
) -> (
  Table,
  Vec<ToolInfo>,
  HashSet<&'static str>,
  HashSet<&'static str>,
) {
  let mut cache: HashMap<&'static str, ToolLookupResult> = HashMap::new();
  let mut missing_unique_tools: Vec<ToolInfo> = Vec::new();
  let mut installed_unique_tools = HashSet::new();
  let mut outdated_unique_tools = HashSet::new();

  let mut doctor_table = Table::new(vec![
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(8)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(20)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2).padding(0, 1));

  for surface in surfaces {
    let resolved = config.resolve_for_lang(surface.name());
    let tools = surface.tool_info(&resolved);

    for tool in tools {
      let lookup = cache
        .entry(tool.binary)
        .or_insert_with(|| lookup_tool_info(tool.binary));

      if lookup.is_installed {
        if installed_unique_tools.insert(tool.binary) {
          let path_str = lookup.path.as_deref().unwrap_or("");
          match &lookup.status {
            Some(ToolStatus::Outdated { current, minimum }) => {
              outdated_unique_tools.insert(tool.binary);
              let v_info = format!(" (v{current} < MSTV v{minimum})");
              let row = Row::new(vec![
                Cell::styled("[WARN] ", Style::Warn),
                Cell::styled(tool.binary, Style::Warn),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Warn),
                ]),
              ]);
              doctor_table.add_row(row);
            }
            Some(ToolStatus::Compatible { current, .. }) => {
              let v_info = format!(" (v{current})");
              let row = Row::new(vec![
                Cell::styled("[READY]", Style::Ok),
                Cell::styled(tool.binary, Style::Tool),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Info),
                ]),
              ]);
              doctor_table.add_row(row);
            }
            _ => {
              let v_info = if let Some(ref v) = lookup.parsed_version {
                format!(" (v{v})")
              } else if let Some(ref v) = lookup.raw_version {
                format!(" ({})", v.trim())
              } else {
                String::new()
              };
              let row = Row::new(vec![
                Cell::styled("[READY]", Style::Ok),
                Cell::styled(tool.binary, Style::Tool),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Info),
                ]),
              ]);
              doctor_table.add_row(row);
            }
          }
        }
      } else if !missing_unique_tools.iter().any(|t| t.binary == tool.binary) {
        missing_unique_tools.push(tool.clone());

        let row = Row::new(vec![
          Cell::styled("[MISS] ", Style::Warn),
          Cell::styled(tool.binary, Style::Warn),
          Cell::styled(surface.name(), Style::Dim),
          Cell::styled(tool.description, Style::Dim),
        ]);
        doctor_table.add_row(row);
      }
    }
  }

  (
    doctor_table,
    missing_unique_tools,
    installed_unique_tools,
    outdated_unique_tools,
  )
}

fn print_unconfigured_languages(
  root: &Path,
  config: &FormalityConfig,
  separator: &str,
) {
  if let Some(ref explicit_langs) = config.resolve_global().languages {
    let mut unconfigured = Vec::new();
    for surface in all_surfaces() {
      if !explicit_langs.iter().any(|l| {
        l.eq_ignore_ascii_case(surface.name())
          || surface.aliases().iter().any(|a| a.eq_ignore_ascii_case(l))
      }) && surface.detect(root)
      {
        unconfigured.push(surface.name());
      }
    }

    if !unconfigured.is_empty() {
      println!("\n{}", separator.dimmed());
      println!("{}", "Unconfigured Workspace Languages:".yellow().bold());
      for name in unconfigured {
        println!(
          "  • Files for '{}' exist in workspace, but '{}' is not in global.languages",
          name.bold(),
          name
        );
      }
      println!(
        "    {} Add them to {} if you want formality to manage them.",
        "Tip:".cyan().bold(),
        "languages = [...]".bold()
      );
    }
  }
}

fn print_virtualenv_status(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
  show_all: bool,
  separator: &str,
) {
  let has_python = surfaces
    .iter()
    .any(|s| s.name() == "python" || s.aliases().contains(&"py"))
    || root.join("pyproject.toml").is_file()
    || root.join("requirements.txt").is_file()
    || root.join("setup.py").is_file()
    || root.join("Pipfile").is_file();
  let venv_info = detect_virtualenv(root);
  if has_python || venv_info.venv_path.is_some() || show_all {
    println!("\n{}", separator.dimmed());
    println!("{}", "Python Virtual Environment:".bold().cyan());
    match &venv_info.source {
      VirtualEnvSource::EnvVar => {
        let p = venv_info
          .venv_path
          .as_ref()
          .map(|p| p.display().to_string())
          .unwrap_or_default();
        println!(
          "  • {} Active virtualenv via VIRTUAL_ENV: {}",
          "[ACTIVE]".green().bold(),
          p.cyan()
        );
      }
      VirtualEnvSource::Workspace(dir_name) => {
        let p = venv_info
          .venv_path
          .as_ref()
          .map(|p| p.display().to_string())
          .unwrap_or_default();
        println!(
          "  • {} Detected workspace virtualenv ({}): {}",
          "[FOUND] ".cyan().bold(),
          dir_name.bold(),
          p.dimmed()
        );
      }
      VirtualEnvSource::None => {
        println!(
          "  • {} No virtual environment detected",
          "[NONE]  ".dimmed()
        );
      }
    }

    if let Some(ref interp) = venv_info.interpreter_path {
      println!(
        "  • Python interpreter: {}",
        interp.display().to_string().cyan()
      );
    } else {
      println!(
        "  • {} No Python interpreter found on PATH or in virtualenv",
        "[WARN] ".yellow().bold()
      );
    }
  }
}

fn print_gitignore_hygiene(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
  separator: &str,
) {
  let hygiene_report = check_gitignore_hygiene(root, surfaces);
  if !hygiene_report.issues.is_empty() {
    println!("\n{}", separator.dimmed());
    println!("{}", "Gitignore Cache Hygiene:".yellow().bold());
    if !hygiene_report.gitignore_exists {
      println!(
        "  • {} No {} file found in workspace root",
        "[WARN] ".yellow().bold(),
        ".gitignore".bold()
      );
    }
    for issue in &hygiene_report.issues {
      let missing_list = issue.missing_patterns.join(", ");
      println!(
        "  • {} {} cache/artifact entries missing from {}: {}",
        "[WARN] ".yellow().bold(),
        issue.category.bold(),
        ".gitignore".bold(),
        missing_list.yellow().bold()
      );
    }
    println!(
      "    {} Add missing patterns to {} to prevent committing artifacts.",
      "Tip:".cyan().bold(),
      ".gitignore".bold()
    );
  }
}

#[cfg(test)]
mod tests;
