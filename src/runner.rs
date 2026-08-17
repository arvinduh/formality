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

    // Execute surfaces concurrently
    let mut results: Vec<SurfaceResult> = surfaces
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

    if let RunnerAction::Sync { check } = action {
      let editorconfig_res =
        crate::editorconfig::sync_editorconfig(root, config, &surfaces, check);
      results.push(editorconfig_res);
    }

    let mut exit_code = 0;
    let mut pass_count = 0;
    let mut violation_count = 0;
    let mut tool_missing_count = 0;
    let mut error_count = 0;

    let mut diagnostics: Vec<(String, String)> = Vec::new();

    let mut runner_table = crate::table::Table::new(vec![
      crate::table::Column::new(crate::table::Cell::text(""))
        .width(crate::table::WidthPolicy::Fixed(8)),
      crate::table::Column::new(crate::table::Cell::text(""))
        .width(crate::table::WidthPolicy::Fixed(14)),
      crate::table::Column::new(crate::table::Cell::text(""))
        .width(crate::table::WidthPolicy::Auto),
      crate::table::Column::new(crate::table::Cell::text(""))
        .align(crate::table::Align::Right)
        .width(crate::table::WidthPolicy::Fixed(12)),
    ])
    .layout(crate::table::Layout::compact().indent(2).padding(0, 1));

    for res in &results {
      let duration_str = format!("{:.2?}", res.duration);

      match &res.status {
        SurfaceStatus::Passed => {
          pass_count += 1;
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[PASS] ", crate::table::Style::Ok),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(
              "Clean / Formatted",
              crate::table::Style::Dim,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
        }
        SurfaceStatus::ConfigSynced { file, created } => {
          pass_count += 1;
          let detail = if *created {
            format!("Created {}", file)
          } else {
            format!("Synced {}", file)
          };
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[SYNC] ", crate::table::Style::Ok),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(detail, crate::table::Style::Info),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
        }
        SurfaceStatus::ConfigDrifted { file, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[DRIFT]", crate::table::Style::Warn),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(
              format!("{} out of sync", file),
              crate::table::Style::Warn,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
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
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[MANUAL]", crate::table::Style::Warn),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(
              format!("{} is manually managed", file),
              crate::table::Style::Warn,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
          diagnostics.push((res.surface_name.to_string(), suggestion.clone()));
        }
        SurfaceStatus::ViolationsFound { message, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[FAIL] ", crate::table::Style::Error),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(
              "Violations found",
              crate::table::Style::Error,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
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
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[MISS] ", crate::table::Style::Warn),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(
              format!("Missing binary: {}", binary),
              crate::table::Style::Warn,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
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
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[ERR]  ", crate::table::Style::Error),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Strong,
            ),
            crate::table::Cell::styled(
              "Execution error",
              crate::table::Style::Error,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
          diagnostics.push((res.surface_name.to_string(), message.clone()));
        }
        SurfaceStatus::Skipped { reason } => {
          runner_table.add_row(crate::table::Row::new(vec![
            crate::table::Cell::styled("[SKIP] ", crate::table::Style::Dim),
            crate::table::Cell::styled(
              res.surface_name,
              crate::table::Style::Dim,
            ),
            crate::table::Cell::styled(
              reason.clone(),
              crate::table::Style::Dim,
            ),
            crate::table::Cell::styled(duration_str, crate::table::Style::Dim)
              .align(crate::table::Align::Right),
          ]));
        }
      }
    }

    let palette = crate::table::Palette::detect();
    let rendered_table = crate::table::render(&runner_table, &palette);
    let separator = crate::table::separator_for_content(&rendered_table);

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
    println!("{}", separator.dimmed());
    if !rendered_table.is_empty() {
      println!("{}", rendered_table);
    }

    if !diagnostics.is_empty() {
      println!("\n{}", separator.dimmed());
      println!("{}", "Diagnostics & Suggestions:".bold());
      for (surface, detail) in diagnostics {
        println!("\n  {} {}", "::".cyan().bold(), surface.bold().magenta());
        for line in detail.lines() {
          println!("    {}", line);
        }
      }
    }

    println!("{}", separator.dimmed());
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
