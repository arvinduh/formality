use crate::config::FormalityConfig;
use crate::surfaces::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus,
};
use colored::Colorize;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerAction {
  Format { check: bool },
  Lint { fix: bool },
  Sync { check: bool },
  Fix,
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
      RunnerAction::Fix => "fix",
    };

    // Execute surfaces concurrently
    let mut results: Vec<SurfaceResult> = match action {
      RunnerAction::Fix => {
        // Stage 1: Run lint(fix: true) across matched surfaces
        let lint_results: Vec<SurfaceResult> = surfaces
          .par_iter()
          .map(|surface| {
            let lang_config = config.resolve_for_lang(surface.name());
            let ctx = ExecutionContext {
              root: root.to_path_buf(),
              paths: paths.to_vec(),
              global_config: global_config.clone(),
              lang_config,
              check_only: false,
            };
            surface.lint(&ctx, true)
          })
          .collect();

        // Stage 2: Run format(check: false) across matched surfaces
        let fmt_results: Vec<SurfaceResult> = surfaces
          .par_iter()
          .map(|surface| {
            let lang_config = config.resolve_for_lang(surface.name());
            let ctx = ExecutionContext {
              root: root.to_path_buf(),
              paths: paths.to_vec(),
              global_config: global_config.clone(),
              lang_config,
              check_only: false,
            };
            surface.format(&ctx)
          })
          .collect();

        // Merge results across both stages per surface
        lint_results
          .into_iter()
          .zip(fmt_results)
          .map(|(lint_res, fmt_res)| combine_fix_results(lint_res, fmt_res))
          .collect()
      }
      _ => surfaces
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
              RunnerAction::Fix => false,
            },
          };

          match action {
            RunnerAction::Format { .. } => surface.format(&ctx),
            RunnerAction::Lint { fix } => surface.lint(&ctx, fix),
            RunnerAction::Sync { check } => surface.sync_config(&ctx, check),
            RunnerAction::Fix => unreachable!(),
          }
        })
        .collect(),
    };

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

    let mut runner_table = crate::ui::table::Table::new(vec![
      crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
        .width(crate::ui::table::WidthPolicy::Fixed(8)),
      crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
        .width(crate::ui::table::WidthPolicy::Fixed(14)),
      crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
        .width(crate::ui::table::WidthPolicy::Auto),
      crate::ui::table::Column::new(crate::ui::table::Cell::text(""))
        .align(crate::ui::table::Align::Right)
        .width(crate::ui::table::WidthPolicy::Fixed(12)),
    ])
    .layout(crate::ui::table::Layout::compact().indent(2).padding(0, 1));

    for res in &results {
      let duration_str = format!("{:.2?}", res.duration);

      match &res.status {
        SurfaceStatus::Passed => {
          pass_count += 1;
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[PASS] ", crate::ui::table::Style::Ok),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              "Clean / Formatted",
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
          ]));
        }
        SurfaceStatus::ConfigSynced { file, created } => {
          pass_count += 1;
          let detail = if *created {
            format!("Created {}", file)
          } else {
            format!("Synced {}", file)
          };
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[SYNC] ", crate::ui::table::Style::Ok),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(detail, crate::ui::table::Style::Info),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
          ]));
        }
        SurfaceStatus::ConfigDrifted { file, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[DRIFT]", crate::ui::table::Style::Warn),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              format!("{} out of sync", file),
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
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
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[MANUAL]", crate::ui::table::Style::Warn),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              format!("{} is manually managed", file),
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
          ]));
          diagnostics.push((res.surface_name.to_string(), suggestion.clone()));
        }
        SurfaceStatus::ViolationsFound { message, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[FAIL] ", crate::ui::table::Style::Error),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              "Violations found",
              crate::ui::table::Style::Error,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
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
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[MISS] ", crate::ui::table::Style::Warn),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              format!("Missing binary: {}", binary),
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
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
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[ERR]  ", crate::ui::table::Style::Error),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              "Execution error",
              crate::ui::table::Style::Error,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
          ]));
          diagnostics.push((res.surface_name.to_string(), message.clone()));
        }
        SurfaceStatus::Skipped { reason } => {
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled("[SKIP] ", crate::ui::table::Style::Dim),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(
              reason.clone(),
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(duration_str, crate::ui::table::Style::Dim)
              .align(crate::ui::table::Align::Right),
          ]));
        }
      }
    }

    let palette = crate::ui::table::Palette::detect();
    let rendered_table = crate::ui::table::render(&runner_table, &palette);
    let separator = crate::ui::table::separator_for_content(&rendered_table);

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

fn combine_fix_results(
  lint_res: SurfaceResult,
  fmt_res: SurfaceResult,
) -> SurfaceResult {
  let surface_name = lint_res.surface_name;
  let duration = lint_res.duration + fmt_res.duration;

  let status = match (lint_res.status, fmt_res.status) {
    // 1. Execution errors take highest precedence
    (
      SurfaceStatus::ExecutionError { message: m1 },
      SurfaceStatus::ExecutionError { message: m2 },
    ) => SurfaceStatus::ExecutionError {
      message: format!(
        "{}
{}",
        m1, m2
      ),
    },
    (SurfaceStatus::ExecutionError { message }, _)
    | (_, SurfaceStatus::ExecutionError { message }) => {
      SurfaceStatus::ExecutionError { message }
    }

    // 2. Missing tool binary
    (
      SurfaceStatus::ToolMissing {
        binary,
        install_hint,
      },
      _,
    )
    | (
      _,
      SurfaceStatus::ToolMissing {
        binary,
        install_hint,
      },
    ) => SurfaceStatus::ToolMissing {
      binary,
      install_hint,
    },

    // 3. Violations found (e.g. unfixable lint errors or formatting errors)
    (
      SurfaceStatus::ViolationsFound {
        message: m1,
        diff: d1,
      },
      SurfaceStatus::ViolationsFound {
        message: m2,
        diff: d2,
      },
    ) => {
      let combined_msg = format!(
        "{}
{}",
        m1, m2
      );
      let combined_diff = match (d1, d2) {
        (Some(a), Some(b)) => Some(format!(
          "{}
{}",
          a, b
        )),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
      };
      SurfaceStatus::ViolationsFound {
        message: combined_msg,
        diff: combined_diff,
      }
    }
    (SurfaceStatus::ViolationsFound { message, diff }, _)
    | (_, SurfaceStatus::ViolationsFound { message, diff }) => {
      SurfaceStatus::ViolationsFound { message, diff }
    }

    // 4. Config drift or manual config
    (SurfaceStatus::ConfigDrifted { file, diff }, _)
    | (_, SurfaceStatus::ConfigDrifted { file, diff }) => {
      SurfaceStatus::ConfigDrifted { file, diff }
    }
    (SurfaceStatus::ManualConfig { file, suggestion }, _)
    | (_, SurfaceStatus::ManualConfig { file, suggestion }) => {
      SurfaceStatus::ManualConfig { file, suggestion }
    }

    // 5. Passed (both passed, or one passed and one was skipped)
    (SurfaceStatus::Passed, SurfaceStatus::Passed)
    | (SurfaceStatus::Passed, SurfaceStatus::Skipped { .. })
    | (SurfaceStatus::Skipped { .. }, SurfaceStatus::Passed) => {
      SurfaceStatus::Passed
    }

    // 6. ConfigSynced
    (SurfaceStatus::ConfigSynced { file, created }, _)
    | (_, SurfaceStatus::ConfigSynced { file, created }) => {
      SurfaceStatus::ConfigSynced { file, created }
    }

    // 7. Both skipped
    (
      SurfaceStatus::Skipped { reason: r1 },
      SurfaceStatus::Skipped { reason: r2 },
    ) => SurfaceStatus::Skipped {
      reason: format!("{}; {}", r1, r2),
    },
  };

  SurfaceResult {
    surface_name,
    status,
    duration,
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

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  #[test]
  fn test_combine_fix_results_passed_and_skipped() {
    let lint_res = SurfaceResult {
      surface_name: "yaml",
      status: SurfaceStatus::Skipped {
        reason: "Tool does not support autofix".to_string(),
      },
      duration: Duration::from_millis(10),
    };
    let fmt_res = SurfaceResult {
      surface_name: "yaml",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(20),
    };

    let combined = combine_fix_results(lint_res, fmt_res);
    assert_eq!(combined.surface_name, "yaml");
    assert_eq!(combined.duration, Duration::from_millis(30));
    assert!(matches!(combined.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_combine_fix_results_both_passed() {
    let lint_res = SurfaceResult {
      surface_name: "python",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(15),
    };
    let fmt_res = SurfaceResult {
      surface_name: "python",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(25),
    };

    let combined = combine_fix_results(lint_res, fmt_res);
    assert_eq!(combined.surface_name, "python");
    assert_eq!(combined.duration, Duration::from_millis(40));
    assert!(matches!(combined.status, SurfaceStatus::Passed));
  }

  #[test]
  fn test_combine_fix_results_violations_precedence() {
    let lint_res = SurfaceResult {
      surface_name: "rust",
      status: SurfaceStatus::ViolationsFound {
        message: "warning: unused".to_string(),
        diff: None,
      },
      duration: Duration::from_millis(50),
    };
    let fmt_res = SurfaceResult {
      surface_name: "rust",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(30),
    };

    let combined = combine_fix_results(lint_res, fmt_res);
    assert!(matches!(
      combined.status,
      SurfaceStatus::ViolationsFound { message, .. } if message.contains("warning: unused")
    ));
  }

  #[test]
  fn test_combine_fix_results_tool_missing_precedence() {
    let lint_res = SurfaceResult {
      surface_name: "python",
      status: SurfaceStatus::ToolMissing {
        binary: "ruff".to_string(),
        install_hint: "pip install ruff".to_string(),
      },
      duration: Duration::from_millis(5),
    };
    let fmt_res = SurfaceResult {
      surface_name: "python",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(5),
    };

    let combined = combine_fix_results(lint_res, fmt_res);
    assert!(matches!(
      combined.status,
      SurfaceStatus::ToolMissing { binary, .. } if binary == "ruff"
    ));
  }

  #[test]
  fn test_combine_fix_results_execution_error_precedence() {
    let lint_res = SurfaceResult {
      surface_name: "cpp",
      status: SurfaceStatus::ExecutionError {
        message: "clang-tidy crashed".to_string(),
      },
      duration: Duration::from_millis(10),
    };
    let fmt_res = SurfaceResult {
      surface_name: "cpp",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(10),
    };

    let combined = combine_fix_results(lint_res, fmt_res);
    assert!(matches!(
      combined.status,
      SurfaceStatus::ExecutionError { message } if message.contains("clang-tidy crashed")
    ));
  }
}
