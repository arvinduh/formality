//! `fml doctor` / `fml install` command: probes every surface's required
//! tools against the resolved config, reports version compatibility, and
//! (with `install`) installs whatever's missing, plus workspace hygiene
//! checks ([`gitignore`], [`venv`]).

/// Gitignore workspace hygiene validation.
pub mod gitignore;
/// Python virtual environment detection and hygiene checks.
pub mod venv;

pub use gitignore::{
  GitignoreHygieneIssue, GitignoreHygieneReport, check_gitignore_hygiene,
  check_gitignore_hygiene_content, is_pattern_ignored,
};
pub use venv::{
  VirtualEnvInfo, VirtualEnvSource, detect_virtualenv,
  detect_virtualenv_with_env, find_system_python, find_venv_interpreter,
};

use crate::config::FormalityConfig;
use crate::engine::version::{
  ToolStatus, Version, evaluate_tool_status, get_raw_tool_version,
  minimum_supported_tool_version, normalize_probed_version, probe_tool_version,
};
use crate::surfaces::{
  LanguageSurface, ToolInfo, all_surfaces, check_binary_exists,
  create_tool_command, detect_surfaces_smart, pinned_version_for,
};
use crate::ui::paths::display_path;
use crate::ui::table::{
  Cell, Column, Frame, Layout, Palette, Row, Span, Style, Table, WidthPolicy,
  render,
};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

/// Install a deduplicated list of missing tools.
///
/// Prints progress to stdout and returns `true` if every tool either
/// installed successfully or already had a known auto-install command,
/// `false` if any tool could not be installed.
#[must_use]
pub fn install_missing_tools(missing: &[ToolInfo]) -> bool {
  // Standalone entry point (`fml fmt/lint/fix --install` preflight): no scan
  // table to size the frame against, so use the plain 80-col cap. This caller
  // prints no tally of its own, so it keeps the pass/fail bit only.
  install_missing_tools_framed(missing, &Frame::capped()).all_ok
}

/// [`install_missing_tools`] rendered inside `frame` so `fml doctor --install`
/// brackets this block with the same rule width as every other section it
/// prints (the "Installing…" progress block, then the Install Summary table).
///
/// Returns the whole per-tool outcome set rather than just a pass/fail bit,
/// because `fml doctor --install`'s closing tally has to be reconciled
/// against what this run actually did (#106).
#[must_use]
fn install_missing_tools_framed(
  missing: &[ToolInfo],
  frame: &Frame,
) -> InstallRunReport {
  if missing.is_empty() {
    return InstallRunReport {
      all_ok: true,
      rows: Vec::new(),
    };
  }

  let palette = Palette::detect();
  println!(
    "\n{}",
    "Installing Missing / Stale Toolchains:".bold().cyan()
  );
  println!("{}", frame.dim_rule(&palette));

  // Bootstrap cargo-binstall once, up front, if any tool here would prefer
  // it and it isn't on PATH yet. Tools like typstyle/tinymist have no real
  // native package on any OS -- cargo-binstall (a prebuilt binary, fetched
  // from the crate's GitHub releases) is their only non-source-compile
  // install path everywhere, including Linux. Without this, a chain would
  // silently skip past it (since `CargoBinstall::is_available()` is false)
  // straight to the `cargo install --locked` source-compile fallback. It's
  // also what lets a pin-carrying `CargoBinstall` entry win over an earlier
  // *available* but unpinned installer (Homebrew's lagging `typstyle` bottle
  // -- #102), which `tool_would_benefit_from_cargo_binstall_bootstrap` now
  // detects.
  //
  // Known limitation: `ensure_cargo_binstall()` needs `cargo` on PATH to
  // bootstrap, so a machine with a package manager but no Rust toolchain at
  // all (e.g. Homebrew-only macOS) can't take this path -- for those the
  // chain still resolves to the lagging system-package install and #102's
  // spurious post-install `[WARN]` can still occur. Giving no-cargo hosts a
  // real prebuilt install path is the broader question tracked in #104.
  if !crate::surfaces::has_cargo_binstall()
    && missing.iter().any(|tool| {
      crate::surfaces::tool_would_benefit_from_cargo_binstall_bootstrap(
        tool.binary,
      )
    })
  {
    println!(
      "\n  {} Bootstrapping {} (prebuilt-binary installer for cargo crates)...",
      "[INSTALL]".cyan().bold(),
      "cargo-binstall".bold()
    );
    if crate::surfaces::ensure_cargo_binstall() {
      println!("    {} cargo-binstall is ready", "[OK]  ".green().bold());
    } else {
      println!(
        "    {} Could not bootstrap cargo-binstall (no cargo, no network, \
         or the install script failed) -- tools that would have used it fall \
         back to the next installer in their chain (a system package manager, \
         or source-compiling with cargo).",
        "[WARN]".yellow().bold()
      );
    }
  }

  let mut all_ok = true;
  let mut summary_rows: Vec<InstallSummaryRow> = Vec::new();

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
          // The preflight scan that put this tool in `missing` already
          // called `check_binary_exists(tool.binary)` and memoized the miss
          // in `BINARY_CACHE` (`surfaces::tooling`). Evict that entry now
          // that the install just succeeded, so every lookup for the rest
          // of this process -- including the `Runner::run` pass that
          // executes right after `install_missing_tools` returns -- sees
          // the binary on `PATH` instead of replaying the stale "not
          // found" result and reporting a tool we just installed as still
          // missing.
          crate::surfaces::forget_binary(tool.binary);

          // Some installers place the binary in a directory this
          // already-running process's `PATH` doesn't contain at all --
          // scoop/winget register the new entry in the Windows registry,
          // and `go install` writes into $GOBIN / $GOPATH/bin. Evicting
          // BINARY_CACHE above isn't enough in those cases, since the
          // underlying `PATH` string genuinely lacks the directory. A
          // no-op for every other installer (npm/cargo/pipx/brew/...,
          // which install alongside a package manager already on `PATH`).
          crate::surfaces::refresh_path_after_install(&program);

          // Resolution guard: an exit code of 0 only proves the package
          // manager ran to completion, never that the binary it produced is
          // reachable from *this* process. `npm i -g prettier` exits 0 with
          // npm's global bin directory absent from `PATH` (routine under
          // nvm, a user-level npm prefix, or a container whose prefix was
          // set after the shell started), and `refresh_path_after_install`
          // is a deliberate no-op for npm. Claiming `[OK]` there would make
          // the doctor footer count a tool as installed that the very next
          // `fml fmt` reports missing -- so ask the same question the scan
          // asks, once, before any row is emitted.
          let on_path = tool_is_on_path(tool.binary);

          // Convergence guard: nor does that exit code prove the binary
          // reports the pinned version — re-probe and warn (once, this
          // invocation) rather than silently claiming success on a tool
          // that would still show `[STALE]` on the very next `fml doctor`.
          // This can only fire for a legitimately misconfigured pin now (an
          // `expected_binary_version` that doesn't actually match what gets
          // installed) -- see `surfaces::tooling::ToolChain`'s doc comment
          // for why most tools deliberately opt out of the pin comparison
          // entirely rather than risk exactly this. Skipped entirely when
          // the binary isn't resolvable: there is nothing to probe, and an
          // unpinned tool must not pay for a probe it never needed.
          let expected = if on_path {
            crate::surfaces::pinned_version_for(tool.binary)
          } else {
            None
          };
          let actual = expected
            .as_ref()
            .and_then(|_| probe_tool_version(tool.binary));

          match classify_install_outcome(
            on_path,
            expected.as_ref(),
            actual.as_ref(),
          ) {
            InstallOutcome::Ok => {
              let detail = expected.map(|v| v.to_string()).unwrap_or_default();
              if detail.is_empty() {
                println!(
                  "    {} Successfully installed {}",
                  "[OK]  ".green().bold(),
                  tool.binary.bold()
                );
              } else {
                println!(
                  "    {} Successfully installed {} ({detail})",
                  "[OK]  ".green().bold(),
                  tool.binary.bold()
                );
              }
              summary_rows.push(InstallSummaryRow {
                binary: tool.binary,
                installer: program.clone(),
                outcome: InstallOutcome::Ok,
                detail,
              });
            }
            InstallOutcome::Warn => {
              let actual_str = actual
                .map(|v| v.to_string())
                .unwrap_or_else(|| "an unparseable version".to_string());
              let expected_str =
                expected.map(|v| v.to_string()).unwrap_or_default();
              println!(
                "    {} Installed {}, but it still reports {actual_str} \
                 (expected {expected_str}) -- the pin for this tool may not \
                 match what the binary itself reports; not retrying \
                 automatically.",
                "[WARN]".yellow().bold(),
                tool.binary.bold()
              );
              all_ok = false;
              summary_rows.push(InstallSummaryRow {
                binary: tool.binary,
                installer: program.clone(),
                outcome: InstallOutcome::Warn,
                detail: format!(
                  "reports {actual_str}, expected {expected_str}"
                ),
              });
            }
            outcome @ InstallOutcome::NotOnPath => {
              println!(
                "    {} {} reported success, but {} is still not on PATH -- \
                 the installer most likely wrote it to a directory this \
                 shell's PATH doesn't contain. Reopen your shell (or add \
                 that directory to PATH) and re-run; treating it as \
                 installed here would only move the failure to the next \
                 command.",
                "[FAIL]".red().bold(),
                program.bold(),
                tool.binary.bold()
              );
              all_ok = false;
              summary_rows.push(InstallSummaryRow {
                binary: tool.binary,
                installer: program.clone(),
                outcome,
                detail: format!("{program} exited 0, but it is not on PATH"),
              });
            }
            // `classify_install_outcome` only ever returns the three above:
            // these two describe an install command that failed or never
            // ran, which this arm has already ruled out.
            InstallOutcome::Fail | InstallOutcome::NoInstaller => {}
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
          summary_rows.push(InstallSummaryRow {
            binary: tool.binary,
            installer: program.clone(),
            outcome: InstallOutcome::Fail,
            detail: format!("exit code {}", status.code().unwrap_or(1)),
          });
        }
        Err(e) => {
          println!(
            "    {} Error running {}: {}",
            "[ERR] ".red().bold(),
            program,
            e
          );
          all_ok = false;
          summary_rows.push(InstallSummaryRow {
            binary: tool.binary,
            installer: program.clone(),
            outcome: InstallOutcome::Fail,
            detail: e.to_string(),
          });
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
      summary_rows.push(InstallSummaryRow {
        binary: tool.binary,
        installer: "-".to_string(),
        outcome: InstallOutcome::NoInstaller,
        detail: tool.install_hint.to_string(),
      });
    }
  }

  // Close the live-progress block, then the Install Summary renders as its
  // own framed section below it.
  println!("{}", frame.dim_rule(&palette));
  print_install_summary_table(&summary_rows, frame);

  InstallRunReport {
    all_ok,
    rows: summary_rows,
  }
}

/// What one [`install_missing_tools_framed`] run did, for a caller that needs
/// more than the pass/fail bit — specifically `fml doctor --install`, whose
/// closing tally is reconciled against these per-tool outcomes rather than by
/// re-probing every tool a second time (#106). Carries the very rows the
/// Install Summary table was rendered from, so the tally and that table can
/// never disagree: there is only one source of truth for both.
struct InstallRunReport {
  /// `true` iff every attempted tool installed cleanly — the value the
  /// pass/fail-only [`install_missing_tools`] entry point still returns.
  all_ok: bool,
  /// One entry per tool this run attempted, in attempt order.
  rows: Vec<InstallSummaryRow>,
}

/// One row of the recap table [`print_install_summary_table`] renders after
/// every tool in a `--install` run has been attempted. Kept separate from
/// the live `[INSTALL]`/`[OK]`/`[FAIL]` lines printed during the loop above
/// -- those are progress output for a run that can take minutes (source
/// compiles included), this is the "what actually happened, at a glance"
/// recap once it's done, the same relationship `fml doctor`'s own scan
/// table has to its live tool-by-tool output.
struct InstallSummaryRow {
  binary: &'static str,
  installer: String,
  outcome: InstallOutcome,
  detail: String,
}

/// Outcome of one tool's install attempt, for [`InstallSummaryRow`].
///
/// `PartialEq`/`Debug` exist so [`classify_install_outcome`]'s decision table
/// can be asserted directly in tests, rather than inferred from rendered
/// output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallOutcome {
  /// Installed and (where a pin exists) confirmed at the expected version.
  Ok,
  /// Installed, but the convergence guard above found its reported version
  /// didn't match the pin.
  Warn,
  /// The installer exited 0, but the binary it claims to have installed is
  /// still not resolvable on `PATH` — so nothing in this process (or the
  /// next command the user runs) can actually invoke it.
  NotOnPath,
  /// The installer command ran and failed, or failed to run at all.
  Fail,
  /// No installer chain entry was available at all -- manual install only.
  NoInstaller,
}

/// Decide what a successful install *command* actually accomplished, from
/// the two facts the install site has just established: whether the binary
/// now resolves on `PATH`, and — for a pinned tool only — what version it
/// reports.
///
/// Split out as a pure function so the case this exists for is unit-testable
/// without spawning a package manager: an installer that exits 0 while the
/// binary never lands on `PATH` must not be reported as installed (#106).
/// `PATH` resolution is checked first and unconditionally, because a tool
/// that cannot be invoked is not "installed at the wrong version", it is
/// absent — and unpinned tools (most of them, per
/// `surfaces::tooling::ToolChain`) would otherwise have no post-install
/// verification at all.
fn classify_install_outcome(
  on_path: bool,
  expected: Option<&Version>,
  probed: Option<&Version>,
) -> InstallOutcome {
  if !on_path {
    return InstallOutcome::NotOnPath;
  }
  match expected {
    Some(expected) if probed != Some(expected) => InstallOutcome::Warn,
    _ => InstallOutcome::Ok,
  }
}

/// Renders and prints the post-install recap table as a framed section, using
/// the caller's `frame` so its rule matches the rest of the command's output:
/// one row per tool this `install_missing_tools` call attempted, its
/// installer, and the outcome. A no-op if `rows` is empty.
fn print_install_summary_table(rows: &[InstallSummaryRow], frame: &Frame) {
  if rows.is_empty() {
    return;
  }

  let mut table = Table::new(vec![
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(20)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(14)),
    Column::new(Cell::text("")).width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2).padding(0, 1).max_width(80));

  for row in rows {
    let (label, style) = match row.outcome {
      InstallOutcome::Ok => ("[OK]  ", Style::Ok),
      InstallOutcome::Warn => ("[WARN]", Style::Warn),
      // Renders like a failure because that is what it is from the caller's
      // side: the tool is not usable. The `detail` column carries the
      // distinction (installer exited 0) that the label can't.
      InstallOutcome::NotOnPath | InstallOutcome::Fail => {
        ("[FAIL]", Style::Error)
      }
      InstallOutcome::NoInstaller => ("[MISS]", Style::Warn),
    };
    table.add_row(Row::new(vec![
      Cell::styled(label, style),
      Cell::styled(row.binary, Style::Tool),
      Cell::styled(row.installer.as_str(), Style::Dim),
      Cell::styled(row.detail.as_str(), Style::Dim),
    ]));
  }

  let palette = Palette::detect();
  let rendered = render(&table, &palette);
  println!(
    "\n{}",
    frame.section(
      &"Install Summary:".bold().cyan().to_string(),
      &rendered,
      &palette,
    )
  );
}

/// Collect the tools required by `surfaces` for the given actions (format and/or
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
  for_lint: bool,
) -> bool {
  let mut seen: HashSet<&'static str> = HashSet::new();
  let mut to_install: Vec<ToolInfo> = Vec::new();
  let global = config.resolve_global();

  for surface in surfaces {
    let resolved = config.resolve_for_lang_with_global(surface.name(), &global);
    for tool in surface.tool_info(&resolved) {
      if seen.contains(tool.binary) {
        continue;
      }
      let needed = (for_fmt && tool.is_required_for_fmt)
        || (for_lint && tool.is_required_for_lint);
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

/// Preflight check for `fml fmt`, `fml lint`, and `fml fix` without `--install`:
/// scans all required tools for the active target surfaces and emits a non-blocking
/// warning to stderr if any tool is present but [`ToolStatus::Stale`] relative to
/// its pinned version.
pub fn preflight_warn_stale_tools(
  surfaces: &[Box<dyn LanguageSurface>],
  config: &FormalityConfig,
  for_fmt: bool,
  for_lint: bool,
) {
  let mut seen: HashSet<&'static str> = HashSet::new();
  let global = config.resolve_global();

  for surface in surfaces {
    let resolved = config.resolve_for_lang_with_global(surface.name(), &global);
    for tool in surface.tool_info(&resolved) {
      if seen.contains(tool.binary) {
        continue;
      }
      let needed = (for_fmt && tool.is_required_for_fmt)
        || (for_lint && tool.is_required_for_lint);
      if !needed {
        continue;
      }
      seen.insert(tool.binary);

      let lookup = lookup_tool_info(tool.binary);
      if lookup.is_installed
        && let Some(ToolStatus::Stale { current, pinned }) =
          lookup.status.as_ref()
      {
        let warning = format_stale_tool_warning(tool.binary, current, pinned);
        eprintln!("{} {warning}", "[WARN]".yellow().bold());
      }
    }
  }
}

/// Formats a preflight warning message for a tool whose installed version is
/// stale relative to its pinned version.
#[must_use]
pub fn format_stale_tool_warning(
  binary: &str,
  current: &Version,
  pinned: &Version,
) -> String {
  format!(
    "tool '{binary}' is stale (v{current} != pinned v{pinned}); run 'fml doctor --install' or pass '--install' to update"
  )
}

/// Pure helper that collects stale tool warning messages for a sequence of
/// (tool_binary_name, status) pairs, deduplicating tool names.
#[must_use]
pub fn collect_stale_tool_warnings<'a>(
  tools: impl IntoIterator<Item = (&'a str, Option<&'a ToolStatus>)>,
) -> Vec<String> {
  let mut warnings = Vec::new();
  let mut seen = HashSet::new();
  for (binary, status) in tools {
    if !seen.insert(binary) {
      continue;
    }
    if let Some(ToolStatus::Stale { current, pinned }) = status {
      warnings.push(format_stale_tool_warning(binary, current, pinned));
    }
  }
  warnings
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

  let scan = scan_tools_and_build_table(root, &surfaces, config);

  let palette = Palette::detect();
  let rendered_table = render(&scan.table, &palette);
  let frame = Frame::for_body(&rendered_table);

  let title = format!(
    "{} {}",
    "fml doctor".bold().cyan(),
    if show_all {
      "(all surfaces)".dimmed()
    } else {
      "(active surfaces)".dimmed()
    }
  );
  println!("{}", frame.section(&title, &rendered_table, &palette));

  // Check for unconfigured surfaces if explicit `languages` is set
  print_unconfigured_languages(root, config, &frame, &palette);

  // Virtual Environment status
  print_virtualenv_status(root, &surfaces, show_all, &frame, &palette);

  // .gitignore Cache Hygiene Check
  print_gitignore_hygiene(root, &surfaces, &frame, &palette);

  // Configuration Schema Version Check
  print_schema_version_check(root, &frame, &palette);

  // Auto-install mode. Genuinely `[MISS]`ing tools and `[STALE]` tools whose
  // selected installer carries a matching pin are reinstalled. If a stale tool's
  // selected installer cannot pin to `expected_binary_version`, reinstall is
  // skipped and explained (#11).
  let mut to_install: Vec<ToolInfo> = scan.missing.clone();
  let mut unpinnable_stale_tools: Vec<(ToolInfo, Version, Version)> =
    Vec::new();

  for tool in &scan.stale {
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
  print_stale_unpinnable_warnings(&unpinnable_stale_tools, &frame, &palette);

  // `fml sync` optionality notice — always prints, so its closing rule is the
  // divider above the summary line below.
  print_sync_notice(&frame, &palette);

  // The closing tally is a value derived from the scan *and then* updated by
  // the install run below — never read back off `scan` again. Keeping it in
  // its own binding is what makes the ordering structural rather than a
  // comment: an install that happens after this point still has somewhere to
  // record itself, whereas the previous code rendered straight from the
  // pre-install snapshot and could only ever contradict the Install Summary
  // table printed a few lines above it (#106).
  let mut tally = ToolTally::from_scan(&scan);

  let mut install_failed = false;
  if install && !to_install.is_empty() {
    let report = install_missing_tools_framed(&to_install, &frame);
    install_failed = !report.all_ok;
    tally.apply_install_run(&report);
  }

  println!("{}\n", tally.render(!to_install.is_empty() && !install));

  if (tally.missing.is_empty() || install) && !install_failed {
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

/// Whether `binary` is reachable from this process right now.
///
/// The single predicate for "is this tool present": both the scan that fills
/// the doctor table ([`lookup_tool_info`]) and the post-install check in
/// [`install_missing_tools_framed`] go through here, so the table, the
/// Install Summary and the closing tally cannot disagree about what
/// "installed" means (#106). [`check_binary_exists`] is a memoized
/// `which::which` — a filesystem lookup, no process spawn — so asking it
/// again after an install costs nothing and is not a second round of version
/// probes.
fn tool_is_on_path(binary: &str) -> bool {
  // The rust surface (and `probe_tool_version`/`get_raw_tool_version`
  // elsewhere in this crate) register/accept the clippy tool under any of
  // these three names — match all of them, not just the literal `"clippy"`
  // that production code never actually passes (`src/surfaces/rust.rs`
  // registers it as `"clippy-driver"`).
  if matches!(binary, "clippy" | "clippy-driver" | "cargo-clippy") {
    clippy_probe_succeeds("clippy-driver", "cargo")
  } else {
    check_binary_exists(binary)
  }
}

fn lookup_tool_info(binary: &'static str) -> ToolLookupResult {
  if tool_is_on_path(binary) {
    let path = which::which(binary)
      .or_else(|_| which::which("clippy-driver"))
      .or_else(|_| which::which("cargo"))
      .ok()
      .map(|p| p.display().to_string());
    let raw_version = get_raw_tool_version(binary);
    let parsed_version = raw_version
      .as_deref()
      .and_then(|raw| normalize_probed_version(binary, raw));
    let mstv = minimum_supported_tool_version(binary);
    let pinned = pinned_version_for(binary);
    // Only fabricate a status when there's an actual floor or pin to check
    // against — a tool with neither (no MSTV entry, no known pin) keeps the
    // prior behavior of `status: None`, rendered as a plain `[READY]` by
    // `scan_tools_and_build_table`'s catch-all arm.
    let status = match parsed_version.clone() {
      Some(curr) => Some(evaluate_tool_status(
        Some(curr),
        raw_version.clone(),
        mstv.as_ref(),
        pinned.as_ref(),
      )),
      None => Some(ToolStatus::UnknownVersion(
        raw_version.clone().unwrap_or_default(),
      )),
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

struct DoctorScanResult {
  table: Table,
  missing: Vec<ToolInfo>,
  installed: HashSet<&'static str>,
  outdated: HashSet<&'static str>,
  stale: Vec<ToolInfo>,
  unknown: HashSet<&'static str>,
}

/// The counts `fml doctor` closes with ("`8 installed (1 unknown), 0
/// missing`"), held apart from [`DoctorScanResult`] on purpose.
///
/// The scan is a snapshot of the world *before* `--install` runs; this is the
/// running tally the footer is actually rendered from, so a successful
/// install has a place to land. Rendering the footer directly off the scan is
/// what made `fml install` report tools as missing in the same breath as the
/// Install Summary table listing them `[OK]` (#106) — the counts were
/// structurally the pre-install snapshot and could not have been anything
/// else.
///
/// The three buckets `--install` moves tools between (`installed`, `stale`,
/// `missing`) stay as name sets so reconciliation can be idempotent;
/// `outdated` and `unknown` collapse to their pre-install counts, since
/// neither is what `--install` fixes. That collapse is a deliberate
/// approximation, not an invariant: a tool installed this run whose version
/// turns out to be unprobeable would ideally join `unknown`, and does not —
/// it is counted plainly as installed. #106's stated expected output matches
/// that, so it stays; anything that later needs `unknown` to be exact must
/// widen it back to a name set rather than assume an install cannot touch it.
struct ToolTally {
  installed: HashSet<&'static str>,
  outdated: usize,
  stale: HashSet<&'static str>,
  unknown: usize,
  missing: HashSet<&'static str>,
}

impl ToolTally {
  /// The pre-install tally, straight off the scan.
  fn from_scan(scan: &DoctorScanResult) -> Self {
    Self {
      installed: scan.installed.clone(),
      outdated: scan.outdated.len(),
      stale: scan.stale.iter().map(|tool| tool.binary).collect(),
      unknown: scan.unknown.len(),
      missing: scan.missing.iter().map(|tool| tool.binary).collect(),
    }
  }

  /// Fold one install run's per-tool outcomes into the tally, so the footer
  /// agrees with the Install Summary table rendered from those same rows
  /// (#106). Driven by the outcomes that table already computed — no second
  /// round of version probes.
  ///
  /// Only the tools that actually landed on `PATH` move: a `[FAIL]` or
  /// `[MISS]` row leaves the tally alone, so a tool that could not be
  /// installed stays counted as missing. A reinstalled `[STALE]` tool was
  /// already present and therefore already in `installed`, so dropping it
  /// from `stale` is the whole change — the `insert` is idempotent and never
  /// double-counts.
  fn apply_install_run(&mut self, report: &InstallRunReport) {
    for row in &report.rows {
      match row.outcome {
        InstallOutcome::Ok => {
          self.missing.remove(row.binary);
          self.stale.remove(row.binary);
          self.installed.insert(row.binary);
        }
        // Installed, and confirmed resolvable on `PATH` by the same
        // predicate this scan used — so it stops being missing. But
        // "present at the wrong version" is exactly what `[STALE]` means,
        // so it lands there rather than being quietly counted as a clean
        // install.
        InstallOutcome::Warn => {
          self.missing.remove(row.binary);
          self.installed.insert(row.binary);
          self.stale.insert(row.binary);
        }
        // `NotOnPath` is an installer that exited 0 without producing a
        // usable binary: the tool stays counted as missing, matching the
        // `[FAIL]` row the Install Summary printed for it.
        InstallOutcome::NotOnPath
        | InstallOutcome::Fail
        | InstallOutcome::NoInstaller => {}
      }
    }
  }

  /// The footer line itself. `offer_install_hint` appends the "run
  /// 'fml install'" nudge, which only makes sense on a run that found work to
  /// do and wasn't already `--install`.
  fn render(&self, offer_install_hint: bool) -> String {
    let qualifier = |count: usize, label: &str| {
      if count == 0 {
        String::new()
      } else {
        format!(" ({count} {label})").yellow().to_string()
      }
    };

    format!(
      "  {} installed{}{}{}, {} missing{}",
      self.installed.len().to_string().green().bold(),
      qualifier(self.outdated, "outdated"),
      qualifier(self.stale.len(), "stale"),
      qualifier(self.unknown, "unknown"),
      if self.missing.is_empty() {
        "0".green().bold().to_string()
      } else {
        self.missing.len().to_string().yellow().bold().to_string()
      },
      if offer_install_hint {
        " (run 'fml install' to install missing/stale tools)"
          .dimmed()
          .to_string()
      } else {
        String::new()
      }
    )
  }
}

fn scan_tools_and_build_table(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
  config: &FormalityConfig,
) -> DoctorScanResult {
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
  let mut unknown_unique_tools = HashSet::new();
  let global = config.resolve_global();

  let mut doctor_table = Table::new(vec![
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(20)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2).padding(0, 1).max_width(80));

  for surface in surfaces {
    let resolved = config.resolve_for_lang_with_global(surface.name(), &global);
    let tools = surface.tool_info(&resolved);

    for tool in tools {
      let lookup = cache
        .entry(tool.binary)
        .or_insert_with(|| lookup_tool_info(tool.binary));

      if lookup.is_installed {
        if installed_unique_tools.insert(tool.binary) {
          // A single path value — use the component-wise helper, not the
          // free-text scanner.
          let path_rel = lookup
            .path
            .as_deref()
            .map(|p| display_path(root, Path::new(p)))
            .unwrap_or_default();
          let path_str = path_rel.as_str();
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
            Some(ToolStatus::UnknownVersion(raw)) => {
              unknown_unique_tools.insert(tool.binary);
              let v_info = if raw.trim().is_empty() {
                " (version unprobeable)".to_string()
              } else {
                format!(" ({})", raw.trim())
              };
              let row = Row::new(vec![
                Cell::styled("[UNKNOWN]", Style::Warn),
                Cell::styled(tool.binary, Style::Warn),
                Cell::styled(surface.name(), Style::Dim),
                Cell::new(vec![
                  Span::styled(path_str, Style::Dim),
                  Span::styled(v_info, Style::Warn),
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

  DoctorScanResult {
    table: doctor_table,
    missing: missing_unique_tools,
    installed: installed_unique_tools,
    outdated: outdated_unique_tools,
    stale: stale_unique_tools,
    unknown: unknown_unique_tools,
  }
}

fn print_unconfigured_languages(
  root: &Path,
  config: &FormalityConfig,
  frame: &Frame,
  palette: &Palette,
) {
  let Some(ref explicit_langs) = config.resolve_global().languages else {
    return;
  };
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
  if unconfigured.is_empty() {
    return;
  }

  let mut body = String::new();
  for name in unconfigured {
    let _ = writeln!(
      body,
      "  • Files for '{}' exist in workspace, but '{}' is not in global.languages",
      name.bold(),
      name
    );
  }
  let _ = write!(
    body,
    "    {} Add them to {} if you want formality to manage them.",
    "Tip:".cyan().bold(),
    "languages = [...]".bold()
  );
  println!(
    "{}",
    frame.section(
      &"Unconfigured Workspace Languages:"
        .yellow()
        .bold()
        .to_string(),
      &frame.wrap_body(&body),
      palette,
    )
  );
}

fn print_virtualenv_status(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
  show_all: bool,
  frame: &Frame,
  palette: &Palette,
) {
  let has_python = surfaces
    .iter()
    .any(|s| s.name() == "python" || s.aliases().contains(&"py"))
    || root.join("pyproject.toml").is_file()
    || root.join("requirements.txt").is_file()
    || root.join("setup.py").is_file()
    || root.join("Pipfile").is_file();
  let venv_info = detect_virtualenv(root);
  if !(has_python || venv_info.venv_path.is_some() || show_all) {
    return;
  }

  let venv_path_display = || {
    venv_info
      .venv_path
      .as_ref()
      .map(|p| display_path(root, p))
      .unwrap_or_default()
  };

  let mut body = String::new();
  match &venv_info.source {
    VirtualEnvSource::EnvVar => {
      let _ = writeln!(
        body,
        "  • {} Active virtualenv via VIRTUAL_ENV: {}",
        "[ACTIVE]".green().bold(),
        venv_path_display().cyan()
      );
    }
    VirtualEnvSource::Workspace(dir_name) => {
      let _ = writeln!(
        body,
        "  • {} Detected workspace virtualenv ({}): {}",
        "[FOUND] ".cyan().bold(),
        dir_name.bold(),
        venv_path_display().dimmed()
      );
    }
    VirtualEnvSource::None => {
      let _ = writeln!(
        body,
        "  • {} No virtual environment detected",
        "[NONE]  ".dimmed()
      );
    }
  }

  if let Some(ref interp) = venv_info.interpreter_path {
    let _ = write!(
      body,
      "  • Python interpreter: {}",
      display_path(root, interp).cyan()
    );
  } else {
    let _ = write!(
      body,
      "  • {} No Python interpreter found on PATH or in virtualenv",
      "[WARN] ".yellow().bold()
    );
  }

  println!(
    "{}",
    frame.section(
      &"Python Virtual Environment:".bold().cyan().to_string(),
      &frame.wrap_body(&body),
      palette,
    )
  );
}

fn print_gitignore_hygiene(
  root: &Path,
  surfaces: &[Box<dyn LanguageSurface>],
  frame: &Frame,
  palette: &Palette,
) {
  let hygiene_report = check_gitignore_hygiene(root, surfaces);
  if hygiene_report.issues.is_empty() {
    return;
  }

  let mut body = String::new();
  if !hygiene_report.gitignore_exists {
    let _ = writeln!(
      body,
      "  • {} No {} file found in workspace root",
      "[WARN] ".yellow().bold(),
      ".gitignore".bold()
    );
  }
  for issue in &hygiene_report.issues {
    let missing_list = issue.missing_patterns.join(", ");
    let _ = writeln!(
      body,
      "  • {} {} cache/artifact entries missing from {}: {}",
      "[WARN] ".yellow().bold(),
      issue.category.bold(),
      ".gitignore".bold(),
      missing_list.yellow().bold()
    );
  }
  let _ = write!(
    body,
    "    {} Add missing patterns to {} to prevent committing artifacts.",
    "Tip:".cyan().bold(),
    ".gitignore".bold()
  );
  println!(
    "{}",
    frame.section(
      &"Gitignore Cache Hygiene:".yellow().bold().to_string(),
      &frame.wrap_body(&body),
      palette,
    )
  );
}

fn print_schema_version_check(root: &Path, frame: &Frame, palette: &Palette) {
  let Some(config_path) = crate::config::find_project_config(root) else {
    return;
  };
  let status = crate::config::schema::check_schema_version_file(&config_path);
  let crate::config::schema::SchemaStatus::Stale { version, expected } = status
  else {
    return;
  };
  let filename = config_path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("formality.toml");

  let mut body = String::new();
  let _ = writeln!(
    body,
    "  • {} {} references outdated schema version 's{}' (current: 's{}')",
    "[WARN] ".yellow().bold(),
    filename.bold(),
    version.to_string().yellow().bold(),
    expected.to_string().green().bold()
  );
  let _ = write!(
    body,
    "    {} Update #:schema directive in {} to pin 's{}'.",
    "Tip:".cyan().bold(),
    filename.bold(),
    expected.to_string().bold()
  );
  println!(
    "{}",
    frame.section(
      &"Configuration Schema Version:".yellow().bold().to_string(),
      &frame.wrap_body(&body),
      palette,
    )
  );
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
  frame: &Frame,
  palette: &Palette,
) {
  if unpinnable_stale.is_empty() {
    return;
  }
  let mut body = String::new();
  for (tool, current, expected) in unpinnable_stale {
    let expl = stale_unpinnable_explanation(tool.binary, current, expected);
    let _ = writeln!(body, "  • {} {}", "[WARN] ".yellow().bold(), expl);
  }
  println!(
    "{}",
    frame.section(
      &"Stale Toolchain Version Drift:".yellow().bold().to_string(),
      &frame.wrap_body(&body),
      palette,
    )
  );
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
fn print_sync_notice(frame: &Frame, palette: &Palette) {
  let body = format!(
    "  {} {}\n    {SYNC_NOTICE_DETAIL}",
    "[INFO] ".cyan().bold(),
    SYNC_NOTICE_SUMMARY.dimmed()
  );
  println!(
    "{}",
    frame.section(
      &"fml sync:".bold().cyan().to_string(),
      &frame.wrap_body(&body),
      palette,
    )
  );
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests;
