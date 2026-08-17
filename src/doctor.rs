use crate::config::FormalityConfig;
use crate::surfaces::{
  LanguageSurface, ToolInfo, all_surfaces, create_tool_command,
  detect_surfaces_smart,
};
use crate::version::{
  ToolStatus, Version, check_tool_compatibility, get_raw_tool_version,
  minimum_supported_tool_version, probe_tool_version,
};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Install a deduplicated list of missing tools.
///
/// Prints progress to stdout and returns `true` if every tool either
/// installed successfully or already had a known auto-install command,
/// `false` if any tool could not be installed.
pub fn install_missing_tools(missing: &[ToolInfo]) -> bool {
  if missing.is_empty() {
    return true;
  }

  println!(
    "\n{}",
    "──────────────────────────────────────────────────────────────────"
      .dimmed()
  );
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

pub fn run_doctor(
  root: &Path,
  show_all: bool,
  install: bool,
  config: &FormalityConfig,
) -> i32 {
  println!(
    "{} {}",
    "fml doctor".bold().cyan(),
    if show_all {
      "(all surfaces)".dimmed()
    } else {
      "(active surfaces)".dimmed()
    }
  );
  println!(
    "{}",
    "──────────────────────────────────────────────────────────────────"
      .dimmed()
  );

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

  let mut cache: HashMap<&'static str, ToolLookupResult> = HashMap::new();
  let mut missing_unique_tools: Vec<ToolInfo> = Vec::new();
  let mut installed_unique_tools = HashSet::new();
  let mut outdated_unique_tools = HashSet::new();

  for surface in &surfaces {
    let resolved = config.resolve_for_lang(surface.name());
    let tools = surface.tool_info(&resolved);

    for tool in tools {
      let lookup = cache.entry(tool.binary).or_insert_with(|| {
        let is_installed = which::which(tool.binary).is_ok()
          || (tool.binary == "clippy"
            && (which::which("clippy-driver").is_ok()
              || which::which("cargo").is_ok()));

        if is_installed {
          let path = which::which(tool.binary)
            .or_else(|_| which::which("clippy-driver"))
            .or_else(|_| which::which("cargo"))
            .ok()
            .map(|p| p.display().to_string());
          let raw_version = get_raw_tool_version(tool.binary);
          let parsed_version = probe_tool_version(tool.binary);
          let status = minimum_supported_tool_version(tool.binary)
            .map(|mstv| check_tool_compatibility(tool.binary, &mstv));

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
      });

      if lookup.is_installed {
        if installed_unique_tools.insert(tool.binary) {
          let path_str = lookup.path.as_deref().unwrap_or("");
          match &lookup.status {
            Some(ToolStatus::Outdated { current, minimum }) => {
              outdated_unique_tools.insert(tool.binary);
              let v_info = format!(" (v{} < MSTV v{})", current, minimum);
              println!(
                "  {} {:<16} {:<10} {}{}",
                "[WARN] ".yellow().bold(),
                tool.binary.bold().yellow(),
                surface.name().dimmed(),
                path_str.dimmed(),
                v_info.yellow().bold()
              );
            }
            Some(ToolStatus::Compatible { current, .. }) => {
              let v_info = format!(" (v{})", current);
              println!(
                "  {} {:<16} {:<10} {}{}",
                "[READY]".green().bold(),
                tool.binary.bold(),
                surface.name().dimmed(),
                path_str.dimmed(),
                v_info.cyan()
              );
            }
            _ => {
              let v_info = if let Some(ref v) = lookup.parsed_version {
                format!(" (v{})", v)
              } else if let Some(ref v) = lookup.raw_version {
                format!(" ({})", v.trim())
              } else {
                String::new()
              };
              println!(
                "  {} {:<16} {:<10} {}{}",
                "[READY]".green().bold(),
                tool.binary.bold(),
                surface.name().dimmed(),
                path_str.dimmed(),
                v_info.cyan()
              );
            }
          }
        }
      } else if !missing_unique_tools.iter().any(|t| t.binary == tool.binary) {
        missing_unique_tools.push(tool.clone());
        println!(
          "  {} {:<16} {:<10} {}",
          "[MISS] ".yellow().bold(),
          tool.binary.bold().yellow(),
          surface.name().dimmed(),
          tool.description.dimmed()
        );
      }
    }
  }

  // Check for unconfigured surfaces if explicit `languages` is set
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
      println!(
        "\n{}",
        "──────────────────────────────────────────────────────────────────"
          .dimmed()
      );
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

  // Auto-install mode
  if install && !missing_unique_tools.is_empty() {
    install_missing_tools(&missing_unique_tools);
  }

  println!(
    "{}",
    "──────────────────────────────────────────────────────────────────"
      .dimmed()
  );
  let outdated_str = if !outdated_unique_tools.is_empty() {
    format!(" ({} outdated)", outdated_unique_tools.len())
      .yellow()
      .to_string()
  } else {
    String::new()
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

  if missing_unique_tools.is_empty() || install {
    0
  } else {
    2
  }
}
