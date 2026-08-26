//! `fml doctor` / `fml install` command: probes every surface's required
//! tools against the resolved config, reports version compatibility, and
//! (with `install`) installs whatever's missing, plus workspace hygiene
//! checks ([`gitignore`], [`venv`]).

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
  ToolStatus, Version, evaluate_tool_status, get_raw_tool_version,
  minimum_supported_tool_version, probe_tool_version,
};
use crate::surfaces::{
  LanguageSurface, ToolInfo, all_surfaces, create_tool_command,
  detect_surfaces_smart, pinned_version_for,
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
  println!("{}", "Installing Missing / Stale Toolchains:".bold().cyan());

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
          // Convergence guard: a successful install exit code only proves
          // the package manager *ran* to completion, not that the binary it
          // produced actually reports the pinned version — re-probe and
          // warn (once, this invocation) rather than silently claiming
          // success on a tool that would still show `[STALE]` on the very
          // next `fml doctor`. This can only fire for a legitimately
          // misconfigured pin now (an `expected_binary_version` that
          // doesn't actually match what gets installed) -- see
          // `surfaces::tooling::ToolChain`'s doc comment for why most tools
          // deliberately opt out of the pin comparison entirely rather than
          // risk exactly this.
          if let Some(expected) =
            crate::surfaces::pinned_version_for(tool.binary)
          {
            let actual = probe_tool_version(tool.binary);
            if actual.as_ref() == Some(&expected) {
              println!(
                "    {} Successfully installed {} ({expected})",
                "[OK]  ".green().bold(),
                tool.binary.bold()
              );
            } else {
              let actual_str = actual
                .map(|v| v.to_string())
                .unwrap_or_else(|| "an unparseable version".to_string());
              println!(
                "    {} Installed {}, but it still reports {actual_str} \
                 (expected {expected}) -- the pin for this tool may not \
                 match what the binary itself reports; not retrying \
                 automatically.",
                "[WARN]".yellow().bold(),
                tool.binary.bold()
              );
              all_ok = false;
            }
          } else {
            println!(
              "    {} Successfully installed {}",
              "[OK]  ".green().bold(),
              tool.binary.bold()
            );
          }
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

/// Collect the tools required by `surfaces` for the given action (format or
/// lint) that need installing — genuinely missing, or present but
/// [`ToolStatus::Stale`] *and* the selected installer carries a matching
/// inline pin — then install them. If a stale tool's selected installer
/// cannot pin to `expected_binary_version`, reinstall is skipped with an
/// explanatory warning. Returns `false` if any scheduled tool could not be
/// installed.
#[must_use]
pub fn preflight_install(
  surfaces: &[Box<dyn LanguageSurface>],
  config: &FormalityConfig,
  for_fmt: bool,
) -> bool {
  let mut seen: HashSet<&'static str> = HashSet::new();
  let mut to_install: Vec<ToolInfo> = Vec::new();

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
      if !needed {
        continue;
      }
      seen.insert(tool.binary);

      let lookup = lookup_tool_info(tool.binary);
      let selected_pin =
        crate::surfaces::selected_pinned_version_for(tool.binary);
      if needs_install(
        lookup.is_installed,
        lookup.status.as_ref(),
        selected_pin.as_ref(),
      ) {
        to_install.push(tool);
      } else if lookup.is_installed
        && let Some(ToolStatus::Stale { current, pinned }) =
          lookup.status.as_ref()
      {
        let expl = stale_unpinnable_explanation(tool.binary, current, pinned);
        println!("  {} {}", "[WARN] ".yellow().bold(), expl);
      }
    }
  }

  install_missing_tools(&to_install)
}

/// Whether a tool needs (re)installing: genuinely absent, or present but
/// [`ToolStatus::Stale`] *and* the selected installer carries an inline pin
/// matching `expected_binary_version`. If the tool is stale but the selected
/// installer cannot pin to `expected_binary_version` (e.g. an unpinned system
/// package manager like `brew`), reinstall is skipped to prevent an unresolvable
/// reinstall loop (#11).
/// Split out as a pure function, independent of any subprocess probing, so
/// the reinstall decision itself is unit-testable (see `tests.rs`) without
/// needing a real stale/pinned binary on `PATH`.
#[must_use]
pub fn needs_install(
  is_installed: bool,
  status: Option<&ToolStatus>,
  selected_pin: Option<&Version>,
) -> bool {
  if !is_installed {
    return true;
  }
  match status {
    Some(ToolStatus::Stale { pinned, .. }) => selected_pin == Some(pinned),
    _ => false,
  }
}

/// Result of probing system installation and compatibility for a tool binary.
pub struct ToolLookupResult {
  /// Whether the binary was found on system PATH.
  pub is_installed: bool,
  /// Path to the binary executable if found.
  pub path: Option<String>,
  /// Raw version output string from `--version`.
  pub raw_version: Option<String>,
  /// Parsed semver version structure.
  pub parsed_version: Option<Version>,
  /// Combined status relative to the MSTV floor and the exact version pin
  /// (`None` when neither is registered for this tool).
  pub status: Option<ToolStatus>,
}
use crate::errors::ExitStatus;

/// Executes the `fml doctor` diagnostic command to scan tools, environment, and hygiene.
#[must_use]
pub fn run_doctor(
  root: &Path,
  show_all: bool,
  install: bool,
  config: &FormalityConfig,
) -> ExitStatus {
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
    stale_unique_tools,
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

  // Configuration Schema Version Check
  print_schema_version_check(root, &separator);

  // Auto-install mode. Genuinely `[MISS]`ing tools and `[STALE]` tools whose
  // selected installer carries a matching pin are reinstalled. If a stale tool's
  // selected installer cannot pin to `expected_binary_version`, reinstall is
  // skipped and explained (#11).
  let mut to_install: Vec<ToolInfo> = missing_unique_tools.clone();
  let mut unpinnable_stale_tools: Vec<(ToolInfo, Version, Version)> =
    Vec::new();

  for tool in &stale_unique_tools {
    let expected = crate::surfaces::pinned_version_for(tool.binary);
    let selected_pin =
      crate::surfaces::selected_pinned_version_for(tool.binary);
    if let Some(ref exp) = expected {
      if selected_pin.as_ref() == Some(exp) {
        to_install.push(tool.clone());
      } else {
        let current = crate::engine::version::probe_tool_version(tool.binary)
          .unwrap_or_else(|| Version::new(0, 0, 0));
        unpinnable_stale_tools.push((tool.clone(), current, exp.clone()));
      }
    }
  }

  // Stale tools with unpinnable installers notice
  print_stale_unpinnable_warnings(&unpinnable_stale_tools, &separator);

  // `fml sync` optionality notice
  print_sync_notice(&separator);

  let mut install_failed = false;
  if install && !to_install.is_empty() && !install_missing_tools(&to_install) {
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
  let stale_str = if stale_unique_tools.is_empty() {
    String::new()
  } else {
    format!(" ({} stale)", stale_unique_tools.len())
      .yellow()
      .to_string()
  };
  println!(
    "  {} installed{}{}, {} missing{}\n",
    installed_unique_tools.len().to_string().green().bold(),
    outdated_str,
    stale_str,
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
    if !to_install.is_empty() && !install {
      " (run 'fml install' to install missing/stale tools)"
        .dimmed()
        .to_string()
    } else {
      String::new()
    }
  );

  if (missing_unique_tools.is_empty() || install) && !install_failed {
    ExitStatus::Clean
  } else {
    ExitStatus::Error
  }
}

/// Whether a subprocess invocation's result indicates the tool actually ran
/// and exited successfully — as opposed to merely existing on disk. Split
/// out from [`clippy_probe_succeeds`] so the success decision is
/// unit-testable without spawning any subprocess at all (see `tests.rs`).
#[must_use]
fn command_ran_successfully(
  result: &std::io::Result<std::process::Output>,
) -> bool {
  matches!(result, Ok(output) if output.status.success())
}

/// Whether the `clippy` component is actually installed and functional, by
/// invoking `<driver_bin> --version` and falling back to
/// `<cargo_bin> clippy --version` (mirroring `get_raw_tool_version`'s own
/// clippy fallback) — a bare `which` presence check is not enough, because
/// `clippy-driver` is a rustup shim that exists on disk whenever rustup is
/// installed, regardless of whether the `clippy` component itself is (see
/// #192). Parameterized over the binary names so tests can substitute a
/// real stand-in binary (e.g. `false`) for a broken shim without mutating
/// `PATH`.
#[must_use]
fn clippy_probe_succeeds(driver_bin: &str, cargo_bin: &str) -> bool {
  command_ran_successfully(
    &create_tool_command(driver_bin).arg("--version").output(),
  ) || command_ran_successfully(
    &create_tool_command(cargo_bin)
      .args(["clippy", "--version"])
      .output(),
  )
}

fn lookup_tool_info(binary: &'static str) -> ToolLookupResult {
  // The rust surface (and `probe_tool_version`/`get_raw_tool_version`
  // elsewhere in this crate) register/accept the clippy tool under any of
  // these three names — match all of them, not just the literal `"clippy"`
  // that production code never actually passes (`src/surfaces/rust.rs`
  // registers it as `"clippy-driver"`).
  let is_installed =
    if matches!(binary, "clippy" | "clippy-driver" | "cargo-clippy") {
      clippy_probe_succeeds("clippy-driver", "cargo")
    } else {
      which::which(binary).is_ok()
    };

  if is_installed {
    let path = which::which(binary)
      .or_else(|_| which::which("clippy-driver"))
      .or_else(|_| which::which("cargo"))
      .ok()
      .map(|p| p.display().to_string());
    let raw_version = get_raw_tool_version(binary);
    let parsed_version = probe_tool_version(binary);
    let mstv = minimum_supported_tool_version(binary);
    let pinned = pinned_version_for(binary);
    // Only fabricate a status when there's an actual floor or pin to check
    // against — a tool with neither (no MSTV entry, no known pin) keeps the
    // prior behavior of `status: None`, rendered as a plain `[READY]` by
    // `scan_tools_and_build_table`'s catch-all arm.
    let status = if mstv.is_some() || pinned.is_some() {
      Some(evaluate_tool_status(
        parsed_version.clone(),
        raw_version.clone(),
        mstv.as_ref(),
        pinned.as_ref(),
      ))
    } else {
      None
    };

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
  Vec<ToolInfo>,
) {
  let mut cache: HashMap<&'static str, ToolLookupResult> = HashMap::new();
  let mut missing_unique_tools: Vec<ToolInfo> = Vec::new();
  let mut installed_unique_tools = HashSet::new();
  let mut outdated_unique_tools = HashSet::new();
  // Tools that are present and executable, but whose installed version
  // doesn't match the exact pin `fml install` would install — [`ToolStatus::
  // Stale`]. Kept as a `Vec<ToolInfo>` (not just a name set, like
  // `installed_unique_tools`/`outdated_unique_tools`) because `fml install`
  // needs the full `ToolInfo` to reinstall it, same as `missing_unique_tools`.
  let mut stale_unique_tools: Vec<ToolInfo> = Vec::new();

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
            Some(ToolStatus::Stale { current, pinned }) => {
              if !stale_unique_tools.iter().any(|t| t.binary == tool.binary) {
                stale_unique_tools.push(tool.clone());
              }
              let v_info = format!(" (v{current} != pinned v{pinned})");
              let row = Row::new(vec![
                Cell::styled("[STALE]", Style::Warn),
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
    stale_unique_tools,
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

fn print_schema_version_check(root: &Path, separator: &str) {
  if let Some(config_path) = crate::config::find_project_config(root) {
    let status = crate::config::schema::check_schema_version_file(&config_path);
    if let crate::config::schema::SchemaStatus::Stale { version, expected } =
      status
    {
      let filename = config_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("formality.toml");

      println!("\n{}", separator.dimmed());
      println!("{}", "Configuration Schema Version:".yellow().bold());
      println!(
        "  • {} {} references outdated schema version 's{}' (current: 's{}')",
        "[WARN] ".yellow().bold(),
        filename.bold(),
        version.to_string().yellow().bold(),
        expected.to_string().green().bold()
      );
      println!(
        "    {} Update #:schema directive in {} to pin 's{}'.",
        "Tip:".cyan().bold(),
        filename.bold(),
        expected.to_string().bold()
      );
    }
  }
}

/// Builds the user-facing explanation for why a stale tool was not scheduled
/// for auto-reinstall (#11).
#[must_use]
pub fn stale_unpinnable_explanation(
  binary: &str,
  current: &Version,
  expected: &Version,
) -> String {
  let selected = crate::surfaces::selected_install_method_for(binary);
  let available_desc = match selected {
    Some(m) => format!("the available installer ({})", m.installer_name()),
    None => "no available installer".to_string(),
  };
  let suggestion = match crate::surfaces::pinned_installer_for(binary) {
    Some(installer) => format!(
      "install {installer} to get the exact pinned version, or accept this drift"
    ),
    None => {
      "accept this drift or install the pinned version manually".to_string()
    }
  };
  format!(
    "{binary} is stale (v{current} != pinned v{expected}), but {available_desc} can't pin to v{expected} — {suggestion}."
  )
}

fn print_stale_unpinnable_warnings(
  unpinnable_stale: &[(ToolInfo, Version, Version)],
  separator: &str,
) {
  if unpinnable_stale.is_empty() {
    return;
  }
  println!("\n{}", separator.dimmed());
  println!("{}", "Stale Toolchain Version Drift:".yellow().bold());
  for (tool, current, expected) in unpinnable_stale {
    let expl = stale_unpinnable_explanation(tool.binary, current, expected);
    println!("  • {} {}", "[WARN] ".yellow().bold(), expl);
  }
}

/// The informational message printed by [`print_sync_notice`], exposed as a
/// standalone constant so it can be asserted on directly in tests without
/// capturing stdout.
pub const SYNC_NOTICE_SUMMARY: &str = "`fml sync` is optional for the primary fml fmt / fml lint / VS Code workflow now \
— config is passed inline and the VS Code extension talks to `fml lsp` directly.";

/// The informational message's second line, describing why `fml sync` still
/// exists for editors that aren't wired up to `fml lsp`.
pub const SYNC_NOTICE_DETAIL: &str = "It remains available for other editor integrations that read native config \
files directly (e.g. `.rustfmt.toml`, for editors whose LSP setup expects a \
real file on disk rather than talking to `fml lsp`).";

/// Prints a purely informational notice that `fml sync` is optional for the
/// primary `fml fmt` / `fml lint` / VS Code workflow now that config is
/// passed inline and the VS Code extension talks to `fml lsp` directly. This
/// does not change `fml sync`'s behavior in any way — it still works exactly
/// as before for editor integrations that read native config files directly.
fn print_sync_notice(separator: &str) {
  println!("\n{}", separator.dimmed());
  println!("{}", "fml sync:".bold().cyan());
  println!(
    "  {} {}",
    "[INFO] ".cyan().bold(),
    SYNC_NOTICE_SUMMARY.dimmed()
  );
  println!("    {SYNC_NOTICE_DETAIL}");
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests;
