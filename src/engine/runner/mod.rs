//! [`Runner`]: the single dispatch point for every subcommand that acts
//! across surfaces (`fmt`, `lint`, `sync`, `fix`) — builds one
//! [`ExecutionContext`] per surface and fans out via `rayon::par_iter`. See
//! `docs/style-guide.md` §4 for the `Arc`-sharing pattern its fields follow.
//!
//! Every such subcommand is expressed as a [`Plan`]: an ordered list of
//! [`Pass`]es plus one [`Mode`]. `--check` is the only mode flag in the CLI
//! and selects [`Mode::Report`]; its absence selects [`Mode::Write`].
//! `fml fix --check` therefore needs no execution code of its own — it is
//! `[Lint, Format]` under `Report`, a plan nobody had spelled before.

use crate::config::FormalityConfig;
use crate::surfaces::{
  ExecutionContext, LanguageSurface, SurfaceResult, SurfaceStatus,
};
use colored::Colorize;
use rayon::prelude::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// One unit of work the runner can dispatch to a [`LanguageSurface`].
///
/// A pass is not a command: `fml fix` is two passes, and `fml lint` is one
/// pass that only ever runs in [`Mode::Report`]. Commands are spelled as
/// [`Plan`]s over these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
  /// Lint pass — [`LanguageSurface::lint`].
  Lint,
  /// Format pass — [`LanguageSurface::format`].
  Format,
  /// Native-config sync pass — [`LanguageSurface::sync_config`].
  ConfigSync,
}

/// Whether a [`Plan`] may write to disk.
///
/// This is the single axis `--check` selects, for every command that has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
  /// Report what would change, writing nothing.
  Report,
  /// Apply changes to disk.
  Write,
}

impl Mode {
  /// Returns `true` for [`Mode::Report`].
  #[must_use]
  pub const fn is_report(self) -> bool {
    matches!(self, Self::Report)
  }

  /// Returns `true` for [`Mode::Write`].
  #[must_use]
  pub const fn is_write(self) -> bool {
    matches!(self, Self::Write)
  }
}

/// An ordered list of [`Pass`]es executed under one [`Mode`] — the single
/// shape every surface-acting subcommand is dispatched as.
///
/// Passes run in list order and their per-surface results are folded
/// left-to-right by [`combine_pass_results`], so `[Lint, Format]` reports
/// the lint pass's findings ahead of the format pass's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
  /// The passes to run, in execution order.
  pub passes: Vec<Pass>,
  /// Whether those passes may write to disk.
  pub mode: Mode,
}

impl Plan {
  /// `fml fmt` / `fml fmt --check`.
  #[must_use]
  pub fn fmt(check: bool) -> Self {
    Self {
      passes: vec![Pass::Format],
      mode: mode_for(check),
    }
  }

  /// `fml lint`.
  ///
  /// There is deliberately no writing form: `lint` never writes, which is
  /// why `fml lint --check` is a CLI error rather than a no-op, and why
  /// `fml lint --fix` was removed in favour of [`Plan::fix`].
  #[must_use]
  pub fn lint() -> Self {
    Self {
      passes: vec![Pass::Lint],
      mode: Mode::Report,
    }
  }

  /// `fml fix` / `fml fix --check`.
  #[must_use]
  pub fn fix(check: bool) -> Self {
    Self {
      passes: vec![Pass::Lint, Pass::Format],
      mode: mode_for(check),
    }
  }

  /// `fml sync` / `fml sync --check`.
  #[must_use]
  pub fn sync(check: bool) -> Self {
    Self {
      passes: vec![Pass::ConfigSync],
      mode: mode_for(check),
    }
  }

  /// Returns `true` if this plan runs `pass`.
  #[must_use]
  pub fn includes(&self, pass: Pass) -> bool {
    self.passes.contains(&pass)
  }

  /// The command spelling this plan corresponds to, used for the run banner.
  #[must_use]
  pub fn verb(&self) -> &'static str {
    match (self.passes.as_slice(), self.mode) {
      ([Pass::Format], Mode::Write) => "fmt",
      ([Pass::Format], Mode::Report) => "fmt --check",
      ([Pass::Lint], _) => "lint",
      ([Pass::Lint, Pass::Format], Mode::Write) => "fix",
      ([Pass::Lint, Pass::Format], Mode::Report) => "fix --check",
      ([Pass::ConfigSync], Mode::Write) => "sync",
      ([Pass::ConfigSync], Mode::Report) => "sync --check",
      _ => "run",
    }
  }
}

const fn mode_for(check: bool) -> Mode {
  if check { Mode::Report } else { Mode::Write }
}

use crate::errors::ExitStatus;

/// Orchestrates parallel tool execution across language surfaces.
pub struct Runner;

impl Runner {
  /// Executes `plan`'s passes across the target surfaces, aggregates the
  /// per-surface results, and renders the status table and diagnostics.
  #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
  #[must_use]
  pub fn run(
    surfaces: Vec<Box<dyn LanguageSurface>>,
    root: &Path,
    paths: &[PathBuf],
    plan: &Plan,
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
    let shared_candidates: Option<Arc<Vec<PathBuf>>> = if paths.is_empty() {
      Some(Arc::new(crate::surfaces::walk_candidate_files(
        root,
        &global_config.exclude,
      )))
    } else {
      None
    };
    let shared = SharedRun {
      config,
      root: Arc::new(root.to_path_buf()),
      paths: Arc::new(paths.to_vec()),
      global_config,
      candidate_files: shared_candidates,
    };

    let action_verb = plan.verb();

    // One pass at a time, each fanned out across every surface in parallel.
    // A later pass sees what an earlier one wrote, which is the whole point
    // of `fix`'s ordering: lint fixes first, then format, so the tree is
    // never left lint-fixed-but-unformatted (`.agents/orchestrate.md` §5).
    let mut pass_results: Vec<(Pass, Vec<SurfaceResult>)> = Vec::new();
    for &pass in &plan.passes {
      let results = run_pass(pass, plan.mode, &surfaces, &shared);
      pass_results.push((pass, results));
    }

    // Targeted re-lint (check-only) for surfaces whose lint pass reported
    // violations, when a *writing* plan ran both passes. The format pass
    // runs after the lint pass, so a violation the linter could not
    // auto-fix may already be gone by now (e.g. markdownlint's MD013 long
    // line that prettier then wrapped). Re-checking only those surfaces
    // keeps the common clean case free of a third lint: a surface that
    // passed the lint pass is not re-run, and the format pass's own result
    // stays authoritative for it. Its status supersedes the lint pass's
    // *before* the fold below, so the recheck's verdict is what gets
    // combined with the format result.
    //
    // Deliberately write-mode-only: under `Mode::Report` nothing was
    // written, so a recheck would observe the same tree the lint pass
    // already saw. See the `fml fix --check` note in the README.
    if plan.mode.is_write()
      && plan.includes(Pass::Lint)
      && plan.includes(Pass::Format)
    {
      for (pass, results) in &mut pass_results {
        if *pass != Pass::Lint {
          continue;
        }
        let rechecks: Vec<Option<SurfaceResult>> = surfaces
          .par_iter()
          .zip(results.par_iter())
          .map(|(surface, lint_res)| {
            if matches!(lint_res.status, SurfaceStatus::ViolationsFound { .. })
            {
              let ctx = shared.ctx_for(surface.as_ref(), false);
              Some(surface.lint(&ctx, false))
            } else {
              None
            }
          })
          .collect();

        *results = std::mem::take(results)
          .into_iter()
          .zip(rechecks)
          .map(|(lint_res, recheck)| apply_recheck(lint_res, recheck))
          .collect();
      }
    }

    // Fold each surface's per-pass results left-to-right, in pass order.
    let mut results: Vec<SurfaceResult> = pass_results
      .into_iter()
      .map(|(_, results)| results)
      .reduce(|acc, next| {
        acc
          .into_iter()
          .zip(next)
          .map(|(a, b)| combine_pass_results(a, b))
          .collect()
      })
      .unwrap_or_default();

    if plan.includes(Pass::ConfigSync) {
      let editorconfig_res = crate::surfaces::editorconfig::sync_editorconfig(
        root,
        config,
        &surfaces,
        plan.mode.is_report(),
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
        SurfaceStatus::ConfigSynced { file, created } => {
          pass_count += 1;
          let detail = if *created {
            format!("Created {file}")
          } else {
            format!("Synced {file}")
          };
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
      format!(
        "({} surface{})",
        surfaces.len(),
        if surfaces.len() == 1 { "" } else { "s" }
      )
      .dimmed()
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

/// Runs one [`Pass`] under one [`Mode`] across every surface in parallel.
///
/// [`Mode`] is resolved to the pass's own read-only/writing form here, and
/// nowhere else — this is the single place the mode axis turns into concrete
/// tool invocations:
///
/// | pass | `Report` | `Write` |
/// | --- | --- | --- |
/// | [`Pass::Lint`] | `lint(fix: false)` | `lint(fix: true)` |
/// | [`Pass::Format`] | `format()` with `check_only` | `format()` writing |
/// | [`Pass::ConfigSync`] | `sync_config(check: true)` | `sync_config(check: false)` |
///
/// `ExecutionContext::check_only` is set from the mode for every pass, not
/// just the format pass. Only `format()` implementations read it (verified
/// across all 12 surfaces); `lint()` takes its read-only-ness from the `fix`
/// argument, and the two surfaces whose `lint()` delegates to `format()`
/// (`json`, `typst`) set `check_only` on their own cloned context. So this
/// is a uniform rule with no behavioral difference from the per-action
/// special-casing it replaces.
fn run_pass(
  pass: Pass,
  mode: Mode,
  surfaces: &[Box<dyn LanguageSurface>],
  shared: &SharedRun<'_>,
) -> Vec<SurfaceResult> {
  let check_only = mode.is_report();
  surfaces
    .par_iter()
    .map(|surface| {
      let ctx = shared.ctx_for(surface.as_ref(), check_only);
      match pass {
        Pass::Lint => surface.lint(&ctx, mode.is_write()),
        Pass::Format => surface.format(&ctx),
        Pass::ConfigSync => surface.sync_config(&ctx, check_only),
      }
    })
    .collect()
}

/// The per-invocation values every surface's [`ExecutionContext`] shares.
///
/// All four owned fields are `Arc`-wrapped so the per-surface parallel
/// dispatch (`rayon::par_iter`) clones a refcount instead of deep-copying
/// the workspace root, the candidate path list, the candidate files, or the
/// global config on every one of the (up to 12) surfaces — and now also on
/// every *pass*, since a plan runs its passes in sequence over the same
/// shared values. See `docs/style-guide.md` §4.
struct SharedRun<'a> {
  config: &'a FormalityConfig,
  root: Arc<PathBuf>,
  paths: Arc<Vec<PathBuf>>,
  global_config: Arc<crate::config::ResolvedGlobalConfig>,
  candidate_files: Option<Arc<Vec<PathBuf>>>,
}

impl SharedRun<'_> {
  /// Builds one surface's [`ExecutionContext`] for a pass running with the
  /// given `check_only`.
  fn ctx_for(
    &self,
    surface: &dyn LanguageSurface,
    check_only: bool,
  ) -> ExecutionContext {
    let lang_config = self
      .config
      .resolve_for_lang_with_global(surface.name(), &self.global_config);
    ExecutionContext {
      root: Arc::clone(&self.root),
      paths: Arc::clone(&self.paths),
      global_config: Arc::clone(&self.global_config),
      lang_config,
      check_only,
      candidate_files: self.candidate_files.clone(),
    }
  }
}

/// Applies a post-format lint recheck to a surface's lint-pass result.
///
/// `recheck`, when present, is a check-only lint run performed *after* the
/// format pass for a surface whose lint pass reported violations (see
/// [`Runner::run`]). Its status supersedes the original lint status so a
/// violation the format pass resolved no longer reports `[FAIL]`; its
/// duration is folded in so the reported time still reflects all the work
/// done. A surface that passed the lint pass has no `recheck` and is
/// returned unchanged.
fn apply_recheck(
  lint_res: SurfaceResult,
  recheck: Option<SurfaceResult>,
) -> SurfaceResult {
  match recheck {
    None => lint_res,
    Some(r) => SurfaceResult {
      surface_name: lint_res.surface_name,
      status: r.status,
      duration: lint_res.duration + r.duration,
    },
  }
}

/// Folds two of a surface's per-pass results into one reported status.
///
/// Applied left-to-right over a [`Plan`]'s passes, so for `fix` this merges
/// the lint pass's result with the format pass's exactly as before the
/// `Plan` refactor. Precedence runs errors → missing tool → violations →
/// config drift → passed, and durations always sum.
fn combine_pass_results(
  first: SurfaceResult,
  second: SurfaceResult,
) -> SurfaceResult {
  let surface_name = first.surface_name;
  let duration = first.duration + second.duration;

  let status = match (first.status, second.status) {
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
    (SurfaceStatus::ConfigSynced { file, created }, _)
    | (_, SurfaceStatus::ConfigSynced { file, created }) => {
      SurfaceStatus::ConfigSynced { file, created }
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
/// the raw message is run through [`normalize_diagnostics`], and a rendered
/// diff is appended verbatim below it. Called from both arms in
/// `Runner::run` so identical raw tool output renders identically regardless
/// of which status it landed in.
///
/// **Both halves are rendered when both are present.** A diff used to
/// replace the message outright, which was invisible while only one pass
/// could fail at a time — but `fml fix --check` runs the lint pass and the
/// format pass against the *same* unmodified tree, so on a dirty tree both
/// routinely report, [`combine_pass_results`] merges them into one status
/// carrying a lint message *and* a format diff, and returning only the diff
/// silently dropped the lint findings. The only producer of a diff
/// (`diff_check_via_tempcopy_classified`) always pairs it with an empty
/// message, so a plain `fml fmt --check` renders byte-identically to before.
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
/// loop; see #175, which is blocked on this issue's restructuring.
fn tool_output_detail(message: &str, diff: Option<&str>) -> String {
  let normalized = normalize_diagnostics(message);
  match diff {
    None => normalized,
    Some(d) if normalized.is_empty() => d.to_string(),
    Some(d) => format!("{normalized}\n{d}"),
  }
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests;
