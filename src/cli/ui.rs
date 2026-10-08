//! Terminal output: one plain line per result, plus warnings and errors.
//!
//! Deliberately minimal. Results and reports go to stdout; warnings, errors
//! and the update notice go to stderr.

use std::fmt;
use std::time;

use colored::Colorize;

use fml::engine::doctor;
use fml::engine::version;
use fml::surfaces;

/// Prints `err` as the standard `[ERR]` line on stderr.
pub fn error(err: &impl fmt::Display) {
  eprintln!("{} {err}", "[ERR]".red().bold());
}

/// Prints `message` as a `[WARN]` line on stderr.
pub fn warn(message: &str) {
  eprintln!("{} {message}", "[WARN]".yellow().bold());
}

/// Prints `message` as an `[OK]` line on stdout.
pub fn ok(message: &str) {
  println!("{} {message}", "[OK]".green().bold());
}

/// Prints one line per surface result, the details of every one that needs
/// attention, and a closing count.
pub fn results(results: &[surfaces::SurfaceResult], elapsed: time::Duration) {
  let mut details = Vec::new();
  let mut failed = 0;
  for result in results {
    let (tag, summary, detail) = describe(&result.status);
    if !matches!(
      result.status,
      surfaces::SurfaceStatus::Passed
        | surfaces::SurfaceStatus::Skipped { .. }
        | surfaces::SurfaceStatus::ConfigSynced { .. }
    ) {
      failed += 1;
    }
    println!(
      "{tag} {:<12} {summary} {}",
      result.surface_name,
      format!("{:.2?}", result.duration).dimmed()
    );
    if let Some(detail) = detail {
      details.push((result.surface_name, detail));
    }
  }
  for (name, detail) in details {
    println!("\n{}\n{}", name.bold(), detail.trim_end());
  }
  let passed = results.len() - failed;
  println!(
    "\n{} passed, {} need attention in {elapsed:.2?}",
    passed.to_string().green().bold(),
    if failed == 0 {
      "0".normal()
    } else {
      failed.to_string().red().bold()
    }
  );
}

/// Returns a status's colored tag, one-line summary, and any detail block.
fn describe(
  status: &surfaces::SurfaceStatus,
) -> (colored::ColoredString, String, Option<String>) {
  match status {
    surfaces::SurfaceStatus::Passed => {
      ("[PASS] ".green().bold(), String::new(), None)
    }
    surfaces::SurfaceStatus::Skipped { reason } => {
      ("[SKIP] ".dimmed(), reason.clone(), None)
    }
    surfaces::SurfaceStatus::ViolationsFound { message, diff } => (
      "[FAIL] ".red().bold(),
      "violations found".to_string(),
      Some(match diff {
        Some(diff) => format!("{message}\n{diff}"),
        None => message.clone(),
      }),
    ),
    surfaces::SurfaceStatus::ToolMissing {
      binary,
      install_hint,
    } => (
      "[MISS] ".yellow().bold(),
      format!("{binary} not found; {install_hint}"),
      None,
    ),
    surfaces::SurfaceStatus::ExecutionError { message } => (
      "[ERR]  ".red().bold(),
      "tool failed".to_string(),
      Some(message.clone()),
    ),
    surfaces::SurfaceStatus::ConfigSynced { files } => (
      "[SYNC] ".green().bold(),
      files
        .iter()
        .map(|f| f.file.as_str())
        .collect::<Vec<_>>()
        .join(", "),
      None,
    ),
    surfaces::SurfaceStatus::ConfigDrifted { file, diff } => {
      ("[DRIFT]".yellow().bold(), file.clone(), Some(diff.clone()))
    }
    surfaces::SurfaceStatus::ManualConfig { file, suggestion } => (
      "[MANUAL]".yellow().bold(),
      format!("{file}: {suggestion}"),
      None,
    ),
  }
}

/// Prints one `fml doctor` line for `check`.
pub fn check(check: &doctor::Check) {
  let (tag, detail) = match &check.status {
    version::ToolStatus::NotFound => (
      "[MISS] ".yellow().bold(),
      format!("install: {}", check.tool.effective_install_hint()),
    ),
    version::ToolStatus::Compatible { current, .. } => {
      ("[OK]   ".green().bold(), format!("v{current}"))
    }
    version::ToolStatus::Outdated { current, minimum } => (
      "[OLD]  ".yellow().bold(),
      format!("v{current}, needs at least v{minimum}"),
    ),
    version::ToolStatus::Stale { current, pinned } => (
      "[STALE]".yellow().bold(),
      format!("v{current}, pinned v{pinned}"),
    ),
    version::ToolStatus::UnknownVersion(_) => {
      ("[?]    ".yellow().bold(), "version unknown".to_string())
    }
  };
  println!(
    "{tag} {:<20} {:<11} {detail}",
    check.tool.binary,
    check.surface.dimmed()
  );
}

/// Prints the outcome of installing `binary`.
pub fn install(binary: &str, outcome: &doctor::Install) {
  match outcome {
    doctor::Install::Installed => ok(&format!("installed {binary}")),
    doctor::Install::Mismatch { expected, actual } => warn(&format!(
      "installed {binary}, but it reports {} (pinned v{expected})",
      actual
        .as_ref()
        .map_or_else(|| "an unknown version".to_string(), |v| format!("v{v}"))
    )),
    doctor::Install::NotOnPath { installer } => error(&format!(
      "{installer} installed {binary}, but it is not on PATH; reopen your \
       shell or add the install directory to PATH"
    )),
    doctor::Install::Failed { installer, code } => error(&format!(
      "{installer} failed to install {binary} (exit code {})",
      code.map_or_else(|| "none".to_string(), |c| c.to_string())
    )),
    doctor::Install::Spawn(err) => {
      error(&format!("could not run the installer for {binary}: {err}"));
    }
    doctor::Install::NoInstaller => {
      warn(&format!("no installer for {binary} is available here"));
    }
  }
}

/// Prints the self-update notice to stderr.
pub fn update_available(tag: &str, command: &str) {
  eprintln!(
    "\n{} formality {} is available (current v{})\n  update: {}",
    "[UPDATE]".cyan().bold(),
    tag.green().bold(),
    env!("CARGO_PKG_VERSION"),
    command.cyan()
  );
}
