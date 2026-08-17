use crate::config::FormalityConfig;
use crate::surfaces::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus,
};
use colored::Colorize;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub enum RunnerAction {
  Format { check: bool },
  Lint { fix: bool },
  Sync { check: bool },
}

pub struct Runner;

impl Runner {
  pub fn run(
    surfaces: Vec<Box<dyn LanguageSurface>>,
    root: &Path,
    paths: &[PathBuf],
    action: RunnerAction,
    config: &FormalityConfig,
  ) -> i32 {
    if surfaces.is_empty() {
      println!("{}", "No matching language surfaces found.".yellow());
      return 0;
    }

    let start_time = Instant::now();
    let global_config = config.resolve_global();

    let action_verb = match &action {
      RunnerAction::Format { check } => {
        if *check {
          "fmt --check"
        } else {
          "fmt"
        }
      }
      RunnerAction::Lint { fix } => {
        if *fix {
          "lint --fix"
        } else {
          "lint"
        }
      }
      RunnerAction::Sync { check } => {
        if *check {
          "sync --check"
        } else {
          "sync"
        }
      }
    };

    println!(
      "{} {} {}",
      "fml".bold().cyan(),
      action_verb.bold(),
      format!(
        "({} surface{})",
        surfaces.len(),
        if surfaces.len() == 1 { "" } else { "s" }
      )
      .dimmed()
    );
    println!(
      "{}",
      "──────────────────────────────────────────────────────────────────"
        .dimmed()
    );

    // Execute surfaces concurrently
    let results: Vec<SurfaceResult> = surfaces
      .par_iter()
      .map(|surface| {
        let lang_config = config.resolve_for_lang(surface.name());
        let ctx = ExecutionContext {
          root: root.to_path_buf(),
          paths: paths.to_vec(),
          global_config: global_config.clone(),
          lang_config,
          check_only: match action {
            RunnerAction::Format { check } => check,
            RunnerAction::Lint { .. } => false,
            RunnerAction::Sync { check } => check,
          },
        };

        match action {
          RunnerAction::Format { .. } => surface.format(&ctx),
          RunnerAction::Lint { fix } => surface.lint(&ctx, fix),
          RunnerAction::Sync { check } => surface.sync_config(&ctx, check),
        }
      })
      .collect();

    let mut exit_code = 0;
    let mut pass_count = 0;
    let mut violation_count = 0;
    let mut tool_missing_count = 0;
    let mut error_count = 0;

    let mut diagnostics: Vec<(String, String)> = Vec::new();

    for res in &results {
      let duration_str = format!("{:.2?}", res.duration);

      match &res.status {
        SurfaceStatus::Passed => {
          pass_count += 1;
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[PASS] ".green().bold(),
            res.surface_name.bold(),
            "Clean / Formatted".dimmed(),
            duration_str.dimmed()
          );
        }
        SurfaceStatus::ConfigSynced { file, created } => {
          pass_count += 1;
          let detail = if *created {
            format!("Created {}", file)
          } else {
            format!("Synced {}", file)
          };
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[SYNC] ".green().bold(),
            res.surface_name.bold(),
            detail.cyan(),
            duration_str.dimmed()
          );
        }
        SurfaceStatus::ConfigDrifted { file, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[DRIFT]".magenta().bold(),
            res.surface_name.bold(),
            format!("{} out of sync", file).magenta(),
            duration_str.dimmed()
          );
          diagnostics.push((
            res.surface_name.to_string(),
            format!(
              "Native config '{}' drifted from formality.toml:\n{}",
              file, diff
            ),
          ));
        }
        SurfaceStatus::ManualConfig { file, suggestion } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[MANUAL]".yellow().bold(),
            res.surface_name.bold(),
            format!("{} is manually managed", file).yellow(),
            duration_str.dimmed()
          );
          diagnostics.push((res.surface_name.to_string(), suggestion.clone()));
        }
        SurfaceStatus::ViolationsFound { message, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[FAIL] ".red().bold(),
            res.surface_name.bold(),
            "Violations found".red(),
            duration_str.dimmed()
          );
          let detail = if let Some(d) = diff {
            d.clone()
          } else {
            normalize_diagnostics(message)
          };
          diagnostics.push((res.surface_name.to_string(), detail));
        }
        SurfaceStatus::ToolMissing {
          binary,
          install_hint,
        } => {
          tool_missing_count += 1;
          exit_code = 2;
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[MISS] ".yellow().bold(),
            res.surface_name.bold(),
            format!("Missing binary: {}", binary).yellow(),
            duration_str.dimmed()
          );
          diagnostics.push((
            res.surface_name.to_string(),
            format!(
              "Missing tool binary '{}'.\n  Install hint: {}",
              binary, install_hint
            ),
          ));
        }
        SurfaceStatus::ExecutionError { message } => {
          error_count += 1;
          exit_code = 2;
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[ERR]  ".red().bold(),
            res.surface_name.bold(),
            "Execution error".red(),
            duration_str.dimmed()
          );
          diagnostics.push((res.surface_name.to_string(), message.clone()));
        }
        SurfaceStatus::Skipped { reason } => {
          println!(
            "  {} {:<12} {:<36} {:>10}",
            "[SKIP] ".dimmed(),
            res.surface_name.dimmed(),
            reason.dimmed(),
            duration_str.dimmed()
          );
        }
      }
    }

    if !diagnostics.is_empty() {
      println!(
        "\n{}",
        "──────────────────────────────────────────────────────────────────"
          .dimmed()
      );
      println!("{}", "Diagnostics & Suggestions:".bold());
      for (surface, detail) in diagnostics {
        println!("\n  {} {}", "::".cyan().bold(), surface.bold().magenta());
        for line in detail.lines() {
          println!("    {}", line);
        }
      }
    }

    println!(
      "{}",
      "──────────────────────────────────────────────────────────────────"
        .dimmed()
    );
    let mut parts = Vec::new();
    if pass_count > 0 {
      parts.push(format!("{} passed", pass_count).green().bold().to_string());
    }
    if violation_count > 0 {
      parts.push(
        format!("{} failed", violation_count)
          .red()
          .bold()
          .to_string(),
      );
    }
    if tool_missing_count > 0 {
      parts.push(
        format!(
          "{} missing tool{}",
          tool_missing_count,
          if tool_missing_count == 1 { "" } else { "s" }
        )
        .yellow()
        .bold()
        .to_string(),
      );
    }
    if error_count > 0 {
      parts.push(
        format!(
          "{} error{}",
          error_count,
          if error_count == 1 { "" } else { "s" }
        )
        .red()
        .bold()
        .to_string(),
      );
    }

    let summary_text = if parts.is_empty() {
      "0 surfaces".dimmed().to_string()
    } else {
      parts.join(", ")
    };

    println!("  {} in {:.2?}\n", summary_text, start_time.elapsed());

    exit_code
  }
}

/// Cleans and standardizes raw CLI tool diagnostics into uniform indented lines
fn normalize_diagnostics(raw: &str) -> String {
  let cleaned_lines: Vec<&str> = raw
    .lines()
    .map(|l| l.trim_end())
    .filter(|l| {
      let trimmed = l.trim();
      !trimmed.is_empty()
        && !trimmed.starts_with("Checking formatting...")
        && !trimmed.starts_with("All checks passed!")
    })
    .collect();

  cleaned_lines.join("\n")
}
