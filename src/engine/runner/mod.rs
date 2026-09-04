//! [`Runner`]: the single dispatch point for every subcommand that acts
//! across surfaces (`fmt`, `lint`, `sync`, `fix`) — builds one
//! [`ExecutionContext`] per surface and fans out via `rayon::par_iter`. See
//! `docs/style-guide.md` §4 for the `Arc`-sharing pattern its fields follow.

use crate::config::FormalityConfig;
use crate::surfaces::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus,
};
use colored::Colorize;
use rayon::prelude::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Action type dispatched by the runner across target language surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerAction {
  /// Format files across surfaces.
  Format {
    /// If `true`, check formatting without writing changes to disk.
    check: bool,
  },
  /// Lint files across surfaces.
  Lint {
    /// If `true`, attempt automatic fix application for lint errors.
    fix: bool,
  },
  /// Sync native configuration files across surfaces.
  Sync {
    /// If `true`, check configuration sync state without writing changes.
    check: bool,
  },
  /// Fix lint issues and reformat files across surfaces.
  Fix,
}

use crate::errors::ExitStatus;

/// Orchestrates parallel tool execution across language surfaces.
pub struct Runner;

impl Runner {
  /// Dispatches parallel surface execution across fix/fmt/lint/sync stages, aggregates results, and renders status tables.
  #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
  #[must_use]
  pub fn run(
    surfaces: Vec<Box<dyn LanguageSurface>>,
    root: &Path,
    paths: &[PathBuf],
    action: RunnerAction,
    config: &FormalityConfig,
  ) -> ExitStatus {
    if surfaces.is_empty() {
      println!("{}", "No matching language surfaces found.".yellow());
      return ExitStatus::Clean;
    }

    let start_time = Instant::now();
    // Shared across every surface's ExecutionContext below. All four are
    // wrapped in Arc so the per-surface parallel dispatch (rayon::par_iter)
    // clones a refcount instead of deep-copying the workspace root, the full
    // candidate path list, the candidate files, or the global config on every
    // one of the (up to 12) surfaces per invocation.
    let global_config = Arc::new(config.resolve_global());
    let shared_paths: Arc<Vec<PathBuf>> = Arc::new(paths.to_vec());
    let shared_root = Arc::new(root.to_path_buf());
    let shared_candidates: Option<Arc<Vec<PathBuf>>> = if paths.is_empty() {
      Some(Arc::new(crate::surfaces::walk_candidate_files(
        root,
        &global_config.exclude,
      )))
    } else {
      None
    };

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
            let ctx = build_ctx(
              surface.as_ref(),
              config,
              &shared_root,
              &shared_paths,
              &global_config,
              &shared_candidates,
              false,
            );
            surface.lint(&ctx, true)
          })
          .collect();

        // Stage 2: Run format(check: false) across matched surfaces
        let fmt_results: Vec<SurfaceResult> = surfaces
          .par_iter()
          .map(|surface| {
            let ctx = build_ctx(
              surface.as_ref(),
              config,
              &shared_root,
              &shared_paths,
              &global_config,
              &shared_candidates,
              false,
            );
            surface.format(&ctx)
          })
          .collect();

        // Stage 3: targeted re-lint (check-only) for surfaces whose lint
        // pass reported violations. The format pass runs *after* the lint
        // pass, so a violation the linter could not auto-fix may already be
        // gone by now (e.g. markdownlint's MD013 long line that prettier
        // then wrapped). Re-checking only those surfaces keeps the common
        // clean case free of a third lint: a surface that passed the lint
        // pass is not re-run, and the format pass's own result stays
        // authoritative for it.
        let recheck_results: Vec<Option<SurfaceResult>> = surfaces
          .par_iter()
          .zip(&lint_results)
          .map(|(surface, lint_res)| {
            if matches!(lint_res.status, SurfaceStatus::ViolationsFound { .. })
            {
              let ctx = build_ctx(
                surface.as_ref(),
                config,
                &shared_root,
                &shared_paths,
                &global_config,
                &shared_candidates,
                false,
              );
              Some(surface.lint(&ctx, false))
            } else {
              None
            }
          })
          .collect();

        // Merge results across all stages per surface
        lint_results
          .into_iter()
          .zip(fmt_results)
          .zip(recheck_results)
          .map(|((lint_res, fmt_res), recheck)| {
            combine_fix_results(lint_res, fmt_res, recheck)
          })
          .collect()
      }
      _ => surfaces
        .par_iter()
        .map(|surface| {
          let check_only = match action {
            RunnerAction::Format { check } | RunnerAction::Sync { check } => {
              check
            }
            RunnerAction::Lint { .. } | RunnerAction::Fix => false,
          };
          let ctx = build_ctx(
            surface.as_ref(),
            config,
            &shared_root,
            &shared_paths,
            &global_config,
            &shared_candidates,
            check_only,
          );

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
      let editorconfig_res = crate::surfaces::editorconfig::sync_editorconfig(
        root, config, &surfaces, check,
      );
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
    .layout(
      crate::ui::table::Layout::compact()
        .indent(2)
        .padding(0, 1)
        .max_width(80),
    );

    for res in &results {
      let duration_str = format!("{:.2?}", res.duration);

      match &res.status {
        SurfaceStatus::Passed => {
          pass_count += 1;
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[PASS] ",
              crate::ui::table::Style::Ok,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              "Clean / Formatted",
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
        }
        SurfaceStatus::ConfigSynced { files } => {
          pass_count += 1;
          // Every file the surface wrote is named, not just the last one
          // (#130) — a config created on disk but absent from this row is
          // the worst failure available to a command whose whole job is
          // writing config files.
          let detail = synced_files_detail(files);
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[SYNC] ",
              crate::ui::table::Style::Ok,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              detail,
              crate::ui::table::Style::Info,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
        }
        SurfaceStatus::ConfigDrifted { file, diff } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[DRIFT]",
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              format!("{file} out of sync"),
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
          diagnostics.push((
            res.surface_name.to_string(),
            format!(
              "Native config '{file}' drifted from formality.toml:\n{diff}"
            ),
          ));
        }
        SurfaceStatus::ManualConfig { file, suggestion } => {
          violation_count += 1;
          if exit_code < 1 {
            exit_code = 1;
          }
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[MANUAL]",
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              format!("{file} is manually managed"),
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
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
            crate::ui::table::Cell::styled(
              "[FAIL] ",
              crate::ui::table::Style::Error,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              "Violations found",
              crate::ui::table::Style::Error,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
          let detail = tool_output_detail(message, diff.as_deref());
          diagnostics.push((res.surface_name.to_string(), detail));
        }
        SurfaceStatus::ToolMissing {
          binary,
          install_hint,
        } => {
          tool_missing_count += 1;
          exit_code = 2;
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[MISS] ",
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              format!("Missing binary: {binary}"),
              crate::ui::table::Style::Warn,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
          diagnostics.push((
            res.surface_name.to_string(),
            format!(
              "Missing tool binary '{binary}'.\n  Install hint: {install_hint}"
            ),
          ));
        }
        SurfaceStatus::ExecutionError { message } => {
          error_count += 1;
          exit_code = 2;
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[ERR]  ",
              crate::ui::table::Style::Error,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Strong,
            ),
            crate::ui::table::Cell::styled(
              "Execution error",
              crate::ui::table::Style::Error,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
          diagnostics.push((
            res.surface_name.to_string(),
            tool_output_detail(message, None),
          ));
        }
        SurfaceStatus::Skipped { reason } => {
          runner_table.add_row(crate::ui::table::Row::new(vec![
            crate::ui::table::Cell::styled(
              "[SKIP] ",
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(
              res.surface_name,
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(
              reason.clone(),
              crate::ui::table::Style::Dim,
            ),
            crate::ui::table::Cell::styled(
              duration_str,
              crate::ui::table::Style::Dim,
            )
            .align(crate::ui::table::Align::Right),
          ]));
        }
      }
    }

    let palette = crate::ui::table::Palette::detect();
    let rendered_table = crate::ui::table::render(&runner_table, &palette);
    let frame = crate::ui::table::Frame::for_body(&rendered_table);

    let title = format!(
      "{} {} {}",
      "fml".bold().cyan(),
      action_verb.bold(),
      format!("({})", header_count_label(results.len())).dimmed()
    );
    println!("{}", frame.section(&title, &rendered_table, &palette));

    if !diagnostics.is_empty() {
      let mut body = String::new();
      for (surface, detail) in &diagnostics {
        // Paths under the run root render relative here, via the same helper
        // the table cells use — see `crate::ui::paths`.
        let detail = crate::ui::paths::relativize_text(root, detail);
        let _ = write!(
          body,
          "\n  {} {}\n",
          "::".cyan().bold(),
          surface.bold().magenta()
        );
        for line in detail.lines() {
          let _ = writeln!(body, "    {line}");
        }
      }
      println!(
        "{}",
        frame.section(
          &"Diagnostics & Suggestions:".bold().to_string(),
          &frame.wrap_body(&body),
          &palette,
        )
      );
    }

    let mut parts = Vec::new();
    if pass_count > 0 {
      parts.push(format!("{pass_count} passed").green().bold().to_string());
    }
    if violation_count > 0 {
      parts.push(format!("{violation_count} failed").red().bold().to_string());
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

    ExitStatus::try_from(exit_code).unwrap_or(ExitStatus::Error)
  }
}

/// Renders the parenthesised count in the run header.
///
/// The count is the number of rows the table actually rendered, **not** the
/// number of matched surfaces (#130). The two diverge for `fml sync`, which
/// appends shared-config rows (`.editorconfig`, `.prettierrc.json`) after the
/// per-surface fan-out: counting matched surfaces produced a deterministic
/// off-by-one on every `fml sync` — the header said `1 surface` while two
/// rows printed and the footer said `2 passed`. Every row still names one
/// surface in its second column (the shared passes render as `editorconfig`
/// and `prettier`), so the noun is unchanged.
fn header_count_label(row_count: usize) -> String {
  format!(
    "{row_count} surface{}",
    if row_count == 1 { "" } else { "s" }
  )
}

/// Renders the detail cell of a `[SYNC]` row: every native config file the
/// surface wrote, each labelled by whether it was created or updated in
/// place — `Created .markdownlint.json, Synced .prettierrc.json`.
///
/// A surface may sync several files (#130), so this is a list rather than
/// one filename. Ordering follows the surface's own write order, which is
/// deterministic, so repeated runs render identically.
fn synced_files_detail(files: &[crate::surfaces::SyncedConfigFile]) -> String {
  files
    .iter()
    .map(|f| {
      let verb = if f.created { "Created" } else { "Synced" };
      format!("{verb} {}", f.file)
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn build_ctx(
  surface: &dyn LanguageSurface,
  config: &FormalityConfig,
  root: &Arc<PathBuf>,
  paths: &Arc<Vec<PathBuf>>,
  global_config: &Arc<crate::config::ResolvedGlobalConfig>,
  candidate_files: &Option<Arc<Vec<PathBuf>>>,
  check_only: bool,
) -> ExecutionContext {
  let lang_config =
    config.resolve_for_lang_with_global(surface.name(), global_config);
  ExecutionContext {
    root: Arc::clone(root),
    paths: Arc::clone(paths),
    global_config: Arc::clone(global_config),
    lang_config,
    check_only,
    candidate_files: candidate_files.clone(),
  }
}

/// Merges the per-surface results of `fml fix`'s lint pass and format pass
/// into one reported status.
///
/// `recheck`, when present, is a check-only lint run performed *after* the
/// format pass for a surface whose lint pass reported violations (see the
/// `RunnerAction::Fix` branch in [`Runner::run`]). Its status supersedes the
/// original lint status so a violation the format pass resolved no longer
/// reports `[FAIL]`; its duration is folded into the total so the reported
/// time still reflects all the work done. A surface that passed the lint
/// pass has no `recheck` and its outcome is decided by the format pass alone.
fn combine_fix_results(
  lint_res: SurfaceResult,
  fmt_res: SurfaceResult,
  recheck: Option<SurfaceResult>,
) -> SurfaceResult {
  let surface_name = lint_res.surface_name;
  let recheck_duration =
    recheck.as_ref().map_or(Duration::ZERO, |r| r.duration);
  let duration = lint_res.duration + fmt_res.duration + recheck_duration;
  let lint_status = recheck.map_or(lint_res.status, |r| r.status);

  let status = match (lint_status, fmt_res.status) {
    // 1. Execution errors take highest precedence
    (
      SurfaceStatus::ExecutionError { message: m1 },
      SurfaceStatus::ExecutionError { message: m2 },
    ) => SurfaceStatus::ExecutionError {
      message: format!(
        "{m1}
{m2}"
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
        "{m1}
{m2}"
      );
      let combined_diff = match (d1, d2) {
        (Some(a), Some(b)) => Some(format!(
          "{a}
{b}"
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
    (
      SurfaceStatus::Passed | SurfaceStatus::Skipped { .. },
      SurfaceStatus::Passed,
    )
    | (SurfaceStatus::Passed, SurfaceStatus::Skipped { .. }) => {
      SurfaceStatus::Passed
    }

    // 6. ConfigSynced
    (SurfaceStatus::ConfigSynced { files }, _)
    | (_, SurfaceStatus::ConfigSynced { files }) => {
      SurfaceStatus::ConfigSynced { files }
    }

    // 7. Both skipped
    (
      SurfaceStatus::Skipped { reason: r1 },
      SurfaceStatus::Skipped { reason: r2 },
    ) => SurfaceStatus::Skipped {
      reason: format!("{r1}; {r2}"),
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
    .map(str::trim_end)
    .filter(|l| {
      let trimmed = l.trim();
      !trimmed.is_empty()
        && !trimmed.starts_with("Checking formatting...")
        && !trimmed.starts_with("All checks passed!")
    })
    .collect();

  cleaned_lines.join("\n")
}

/// Shared diagnostic-detail computation for the two `SurfaceStatus` arms that
/// carry raw tool output (`ViolationsFound` and `ExecutionError`, per #146):
/// a rendered diff is shown verbatim, otherwise the raw message is run
/// through [`normalize_diagnostics`]. Called from both arms in `Runner::run`
/// so identical raw tool output renders identically regardless of which
/// status it landed in.
///
/// A diff deliberately bypasses [`normalize_diagnostics`]: that function
/// trims line ends and drops blank lines, which in a diff body are file
/// *contents* — exactly the drift a whitespace diff exists to show. Path
/// relativization is not lost by the bypass; it runs over every detail at
/// the diagnostics-render step, after this function.
///
/// Coverage caveat: the tests pin this function's behavior only. Nothing
/// currently asserts that either arm in `Runner::run` still calls it, so
/// un-wiring a call site — i.e. reintroducing #146 — does not fail any
/// test. Real call-site coverage needs a testable seam in the rendering
/// loop; see the follow-up spun off from #146.
fn tool_output_detail(message: &str, diff: Option<&str>) -> String {
  match diff {
    Some(d) => d.to_string(),
    None => normalize_diagnostics(message),
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests;
