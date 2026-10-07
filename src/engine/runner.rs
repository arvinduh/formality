//! `Runner`: the single dispatch point for every subcommand that acts
//! across surfaces (`fmt`, `lint`, `sync`, `fix`) — builds one
//! `ExecutionContext` per surface and fans out via `rayon::par_iter`. See
//! `docs/style-guide.md` §4 for the `Arc`-sharing pattern its fields follow.
//!
//! Every such subcommand is expressed as a `Plan`: an ordered list of
//! `Pass`es plus one `Mode`. `--check` is the only mode flag in the CLI
//! and selects `Mode::Report`; its absence selects `Mode::Write`.
//! `fml fix --check` therefore needs no execution code of its own — it is
//! `[Lint, Format]` under `Report`, a plan nobody had spelled before.

use std::path;
use std::sync;

use rayon::iter::IndexedParallelIterator;
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;

use crate::config;
use crate::surfaces;
use crate::surfaces::sync::editorconfig;
use crate::surfaces::sync::prettier;

/// One unit of work the runner can dispatch to a [`surfaces::LanguageSurface`].
///
/// A pass is not a command: `fml fix` is two passes, and `fml lint` is one
/// pass that only ever runs in [`Mode::Report`]. Commands are spelled as
/// [`Plan`]s over these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pass {
  /// Lint pass — [`surfaces::LanguageSurface::lint`].
  Lint,
  /// Format pass — [`surfaces::LanguageSurface::format`].
  Format,
  /// Native-config sync pass — [`surfaces::LanguageSurface::sync_config`].
  ConfigSync,
}

/// Whether a [`Plan`] may write to disk.
///
/// This is the single axis `--check` selects, for every command that has it.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
  /// Report what would change, writing nothing.
  Report,
  /// Apply changes to disk.
  Write,
}

impl Mode {
  /// Returns `true` for [`Mode::Report`].
  #[must_use]
  const fn is_report(self) -> bool {
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
/// left-to-right by `combine_pass_results`, so `[Lint, Format]` reports
/// the lint pass's findings ahead of the format pass's.
#[derive(Debug)]
pub struct Plan {
  /// The passes to run, in execution order.
  passes: Vec<Pass>,
  /// Whether those passes may write to disk.
  pub mode: Mode,
  /// Whether a surface reporting [`surfaces::SurfaceStatus::ToolMissing`] alone should
  /// keep the run's exit code clean (#252 / #163). Only `fmt`, `lint`, and
  /// `fix` expose this on the CLI (`--allow-missing`) — `sync` and `doctor`
  /// don't, so [`Plan::sync`] always leaves it `false`. A real violation or
  /// an [`surfaces::SurfaceStatus::ExecutionError`] still exits non-zero regardless of
  /// this flag; it only silences the "missing tool" precondition itself.
  pub allow_missing: bool,
}

impl Plan {
  /// `fml fmt` / `fml fmt --check`, optionally with `--allow-missing`.
  #[must_use]
  pub fn fmt(check: bool, allow_missing: bool) -> Self {
    Self {
      passes: vec![Pass::Format],
      mode: mode_for(check),
      allow_missing,
    }
  }

  /// `fml lint`, optionally with `--allow-missing`.
  ///
  /// There is deliberately no writing form: `lint` never writes, which is
  /// why `fml lint --check` is a CLI error rather than a no-op, and why
  /// `fml lint --fix` was removed in favour of [`Plan::fix`].
  #[must_use]
  pub fn lint(allow_missing: bool) -> Self {
    Self {
      passes: vec![Pass::Lint],
      mode: Mode::Report,
      allow_missing,
    }
  }

  /// `fml fix` / `fml fix --check`, optionally with `--allow-missing`.
  #[must_use]
  pub fn fix(check: bool, allow_missing: bool) -> Self {
    Self {
      passes: vec![Pass::Lint, Pass::Format],
      mode: mode_for(check),
      allow_missing,
    }
  }

  /// `fml sync` / `fml sync --check`.
  ///
  /// No `--allow-missing` form: `sync` never reads formatter/linter
  /// binaries, so it has no `ToolMissing` precondition to opt out of.
  #[must_use]
  pub fn sync(check: bool) -> Self {
    Self {
      passes: vec![Pass::ConfigSync],
      mode: mode_for(check),
      allow_missing: false,
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

/// The files one run acts on, resolved once by the command layer and shared
/// by surface detection and every surface's file selection.
pub enum Scope {
  /// Explicit path arguments, or the files `--staged`/`--changed` selected.
  Paths {
    /// The arguments as given, which some tools are handed directly.
    args: sync::Arc<Vec<path::PathBuf>>,
    /// `args` expanded once into candidate files; each surface filters
    /// its own files from them.
    files: sync::Arc<Vec<path::PathBuf>>,
  },
  /// The whole workspace: every candidate file, `global.exclude` applied,
  /// from one walk.
  Workspace(sync::Arc<Vec<path::PathBuf>>),
}

impl Scope {
  /// Scopes a run to `paths`, expanding each directory among them once, or,
  /// when there are none, to the workspace under `root`, walking it once.
  #[must_use]
  pub fn resolve(
    root: &path::Path,
    paths: &[path::PathBuf],
    global_exclude: &[path::PathBuf],
  ) -> Self {
    if paths.is_empty() {
      Self::Workspace(sync::Arc::new(surfaces::glob::walk_candidate_files(
        root,
        global_exclude,
      )))
    } else {
      Self::Paths {
        args: sync::Arc::new(paths.to_vec()),
        files: sync::Arc::new(surfaces::glob::expand_targets(root, paths)),
      }
    }
  }
}

/// Orchestrates parallel tool execution across language surfaces.
pub struct Runner;

impl Runner {
  /// Executes `plan`'s passes across the target surfaces, returning the
  /// in-memory execution results per surface.
  #[must_use]
  pub fn run(
    surfaces: &[Box<dyn surfaces::LanguageSurface>],
    root: &path::Path,
    scope: &Scope,
    plan: &Plan,
    config: &config::FormalityConfig,
  ) -> Vec<surfaces::SurfaceResult> {
    if surfaces.is_empty() {
      return Vec::new();
    }
    // Shared across every surface's ExecutionContext below. All four are
    // wrapped in Arc so the per-surface parallel dispatch (rayon::par_iter)
    // clones a refcount instead of deep-copying the workspace root, the full
    // candidate path list, the candidate files, or the global config on every
    // one of the (up to 12) surfaces per invocation.
    let global_config = sync::Arc::new(config.resolve_global());
    let (paths, candidate_files) = match scope {
      Scope::Paths { args, files } => {
        (sync::Arc::clone(args), sync::Arc::clone(files))
      }
      Scope::Workspace(files) => {
        (sync::Arc::default(), sync::Arc::clone(files))
      }
    };
    let shared = SharedRun {
      config,
      root: sync::Arc::new(root.to_path_buf()),
      paths,
      global_config,
      candidate_files,
    };

    // One pass at a time, each fanned out across every surface in parallel.
    // A later pass sees what an earlier one wrote, which is the whole point
    // of `fix`'s ordering: lint fixes first, then format, so the tree is
    // never left lint-fixed-but-unformatted (Smart Format, `AGENTS.md`).
    let mut pass_results: Vec<(Pass, Vec<surfaces::SurfaceResult>)> =
      Vec::new();
    for &pass in &plan.passes {
      let results = run_pass(pass, plan.mode, surfaces, &shared);
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
        let rechecks: Vec<Option<surfaces::SurfaceResult>> = surfaces
          .par_iter()
          .zip(results.par_iter())
          .map(|(surface, lint_res)| {
            if matches!(
              lint_res.status,
              surfaces::SurfaceStatus::ViolationsFound { .. }
            ) {
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
    let mut results: Vec<surfaces::SurfaceResult> = pass_results
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
      if let Some(prettier_res) = prettier::sync_shared_prettier_config(
        root,
        config,
        surfaces,
        plan.mode.is_report(),
      ) {
        results.push(prettier_res);
      }
      let editorconfig_res = editorconfig::sync_editorconfig(
        root,
        config,
        surfaces,
        plan.mode.is_report(),
      );
      results.push(editorconfig_res);
    }

    results
  }
}

/// How a run ends, ordered from best to worst; the process exit code is the
/// variant's position (0, 1, 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExitStatus {
  /// Nothing to report.
  Clean,
  /// Violations, drift, or a missing tool.
  Violations,
  /// A tool or fml itself failed to run.
  Error,
}

impl ExitStatus {
  /// Returns `true` for [`ExitStatus::Clean`].
  #[must_use]
  pub const fn is_clean(self) -> bool {
    matches!(self, Self::Clean)
  }
}

/// Folds surface results into the run's exit status: the worst row wins.
#[must_use]
pub fn compute_exit_status(
  results: &[surfaces::SurfaceResult],
  allow_missing: bool,
) -> ExitStatus {
  results
    .iter()
    .map(|res| exit_floor(&res.status.severity(), allow_missing))
    .max()
    .unwrap_or(ExitStatus::Clean)
}

/// The lowest exit code a row of `severity` forces on the run.
///
/// The results-row loop in [`Runner::run`] folds it in with
/// `max`, so the worst row decides the exit code.
fn exit_floor(
  severity: &surfaces::Severity,
  allow_missing: bool,
) -> ExitStatus {
  match severity {
    surfaces::Severity::Skipped | surfaces::Severity::Passed => {
      ExitStatus::Clean
    }
    // An unmet precondition, not an operational fault (#252) — the
    // surface correctly determined it could not proceed. Exit 1
    // (`ExitStatus::Violations`), the same as a real violation, so a
    // missing tool never lets the process exit clean; 2 stays reserved
    // for `surfaces::Severity::Error`, which still wins if one occurs elsewhere.
    //
    // `--allow-missing` (#163) is the one opt-out: a machine missing an
    // optional linter must not fail *every* commit that touches that
    // surface. It only silences this severity's contribution to the
    // exit code — a real violation elsewhere still sets it via its own
    // floor, and the row stays visible either way (silence is the
    // original bug, not the fix). The tally is untouched by this flag:
    // a missing tool is still counted, just not floored into a nonzero
    // exit.
    surfaces::Severity::ToolMissing if allow_missing => ExitStatus::Clean,
    surfaces::Severity::ToolMissing | surfaces::Severity::Violation => {
      ExitStatus::Violations
    }
    surfaces::Severity::Error => ExitStatus::Error,
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
  surfaces: &[Box<dyn surfaces::LanguageSurface>],
  shared: &SharedRun<'_>,
) -> Vec<surfaces::SurfaceResult> {
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
  config: &'a config::FormalityConfig,
  root: sync::Arc<path::PathBuf>,
  paths: sync::Arc<Vec<path::PathBuf>>,
  global_config: sync::Arc<config::ResolvedGlobalConfig>,
  candidate_files: sync::Arc<Vec<path::PathBuf>>,
}

impl SharedRun<'_> {
  /// Builds one surface's [`ExecutionContext`] for a pass running with the
  /// given `check_only`.
  fn ctx_for(
    &self,
    surface: &dyn surfaces::LanguageSurface,
    check_only: bool,
  ) -> surfaces::ExecutionContext {
    let lang_config = self
      .config
      .resolve_for_lang_with_global(surface.name(), &self.global_config);
    surfaces::ExecutionContext {
      root: sync::Arc::clone(&self.root),
      paths: sync::Arc::clone(&self.paths),
      global_config: sync::Arc::clone(&self.global_config),
      lang_config,
      check_only,
      candidate_files: sync::Arc::clone(&self.candidate_files),
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
  lint_res: surfaces::SurfaceResult,
  recheck: Option<surfaces::SurfaceResult>,
) -> surfaces::SurfaceResult {
  match recheck {
    None => lint_res,
    Some(r) => surfaces::SurfaceResult {
      surface_name: lint_res.surface_name,
      status: r.status,
      duration: lint_res.duration + r.duration,
    },
  }
}

/// Folds two of a surface's per-pass results into one reported status.
///
/// Applied left-to-right over a [`Plan`]'s passes, so for `fix` this merges
/// the lint pass's result with the format pass's. The status with the higher
/// [`precedence`] wins and an exact tie keeps `first`, except that two
/// execution errors, two violation reports or two skips merge their text.
/// Durations always sum.
fn combine_pass_results(
  first: surfaces::SurfaceResult,
  second: surfaces::SurfaceResult,
) -> surfaces::SurfaceResult {
  let surface_name = first.surface_name;
  let duration = first.duration + second.duration;

  let status = match (first.status, second.status) {
    (
      surfaces::SurfaceStatus::ExecutionError { message: m1 },
      surfaces::SurfaceStatus::ExecutionError { message: m2 },
    ) => surfaces::SurfaceStatus::ExecutionError {
      message: format!("{m1}\n{m2}"),
    },
    (
      surfaces::SurfaceStatus::ViolationsFound {
        message: m1,
        diff: d1,
      },
      surfaces::SurfaceStatus::ViolationsFound {
        message: m2,
        diff: d2,
      },
    ) => {
      let combined_msg = format!("{m1}\n{m2}");
      let combined_diff = match (d1, d2) {
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
      };
      surfaces::SurfaceStatus::ViolationsFound {
        message: combined_msg,
        diff: combined_diff,
      }
    }
    (
      surfaces::SurfaceStatus::Skipped { reason: r1 },
      surfaces::SurfaceStatus::Skipped { reason: r2 },
    ) => surfaces::SurfaceStatus::Skipped {
      reason: format!("{r1}; {r2}"),
    },
    (first, second) => {
      if precedence(&second) > precedence(&first) {
        second
      } else {
        first
      }
    }
  };

  surfaces::SurfaceResult {
    surface_name,
    status,
    duration,
  }
}

/// Ranks a status for [`combine_pass_results`]: by [`Severity`] first, then,
/// between statuses of one severity, by which carries the more specific
/// report (a tool's violations over config drift over a hand-written config;
/// a config write over a bare pass).
fn precedence(status: &surfaces::SurfaceStatus) -> (surfaces::Severity, u8) {
  let within_severity = match status {
    surfaces::SurfaceStatus::ViolationsFound { .. } => 2,
    surfaces::SurfaceStatus::ConfigDrifted { .. }
    | surfaces::SurfaceStatus::ConfigSynced { .. } => 1,
    surfaces::SurfaceStatus::ManualConfig { .. }
    | surfaces::SurfaceStatus::Passed
    | surfaces::SurfaceStatus::Skipped { .. }
    | surfaces::SurfaceStatus::ToolMissing { .. }
    | surfaces::SurfaceStatus::ExecutionError { .. } => 0,
  };
  (status.severity(), within_severity)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time;

  #[test]
  fn combine_pass_results_passed_and_skipped() {
    let lint_res = surfaces::SurfaceResult {
      surface_name: "yaml",
      status: surfaces::SurfaceStatus::Skipped {
        reason: "Tool does not support autofix".to_string(),
      },
      duration: time::Duration::from_millis(10),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "yaml",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(20),
    };

    let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
    assert_eq!(combined.surface_name, "yaml");
    assert_eq!(combined.duration, time::Duration::from_millis(30));
    assert!(matches!(combined.status, surfaces::SurfaceStatus::Passed));
  }

  #[test]
  fn combine_pass_results_both_passed() {
    let lint_res = surfaces::SurfaceResult {
      surface_name: "python",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(15),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "python",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(25),
    };

    let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
    assert_eq!(combined.surface_name, "python");
    assert_eq!(combined.duration, time::Duration::from_millis(40));
    assert!(matches!(combined.status, surfaces::SurfaceStatus::Passed));
  }

  #[test]
  fn combine_pass_results_recheck_clears_lint_violation() {
    // Issue #116: the lint pass reported a violation, but the post-format
    // re-check came back clean. The re-check supersedes the stale lint status,
    // so the surface reports Passed and its duration folds in all three passes.
    let lint_res = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "MD013/line-length".to_string(),
        diff: None,
      },
      duration: time::Duration::from_millis(40),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(30),
    };
    let recheck = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(20),
    };

    let combined =
      combine_pass_results(apply_recheck(lint_res, Some(recheck)), fmt_res);
    assert!(matches!(combined.status, surfaces::SurfaceStatus::Passed));
    assert_eq!(combined.duration, time::Duration::from_millis(90));
  }

  #[test]
  fn combine_pass_results_recheck_preserves_surviving_violation() {
    // Issue #116 inverse: the violation survived the format pass, so the
    // re-check still reports it and the surface still fails.
    let lint_res = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "MD025/single-title".to_string(),
        diff: None,
      },
      duration: time::Duration::from_millis(40),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(30),
    };
    let recheck = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "MD025/single-title".to_string(),
        diff: None,
      },
      duration: time::Duration::from_millis(20),
    };

    let combined =
      combine_pass_results(apply_recheck(lint_res, Some(recheck)), fmt_res);
    assert!(matches!(
      combined.status,
      surfaces::SurfaceStatus::ViolationsFound { message, .. }
        if message.contains("MD025")
    ));
    assert_eq!(combined.duration, time::Duration::from_millis(90));
  }

  #[test]
  fn combine_pass_results_violations_precedence() {
    let lint_res = surfaces::SurfaceResult {
      surface_name: "rust",
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "warning: unused".to_string(),
        diff: None,
      },
      duration: time::Duration::from_millis(50),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "rust",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(30),
    };

    let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
    assert!(matches!(
      combined.status,
      surfaces::SurfaceStatus::ViolationsFound { message, .. } if message.contains("warning: unused")
    ));
  }

  #[test]
  fn combine_pass_results_tool_missing_precedence() {
    let lint_res = surfaces::SurfaceResult {
      surface_name: "python",
      status: surfaces::SurfaceStatus::ToolMissing {
        binary: "ruff".to_string(),
        install_hint: "pip install ruff".to_string(),
      },
      duration: time::Duration::from_millis(5),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "python",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(5),
    };

    let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
    assert!(matches!(
      combined.status,
      surfaces::SurfaceStatus::ToolMissing { binary, .. } if binary == "ruff"
    ));
  }

  #[test]
  fn combine_pass_results_execution_error_precedence() {
    let lint_res = surfaces::SurfaceResult {
      surface_name: "cpp",
      status: surfaces::SurfaceStatus::ExecutionError {
        message: "clang-tidy crashed".to_string(),
      },
      duration: time::Duration::from_millis(10),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "cpp",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(10),
    };

    let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
    assert!(matches!(
      combined.status,
      surfaces::SurfaceStatus::ExecutionError { message } if message.contains("clang-tidy crashed")
    ));
  }

  /// One status per `surfaces::SurfaceStatus` variant, every payload tagged with `tag`,
  /// ordered from lowest to highest `combine_pass_results` precedence.
  fn every_status_by_precedence(tag: &str) -> Vec<surfaces::SurfaceStatus> {
    vec![
      surfaces::SurfaceStatus::Skipped {
        reason: tag.to_string(),
      },
      surfaces::SurfaceStatus::Passed,
      surfaces::SurfaceStatus::ConfigSynced {
        files: vec![surfaces::SyncedConfigFile::new(tag, true)],
      },
      surfaces::SurfaceStatus::ToolMissing {
        binary: tag.to_string(),
        install_hint: tag.to_string(),
      },
      surfaces::SurfaceStatus::ManualConfig {
        file: tag.to_string(),
        suggestion: tag.to_string(),
      },
      surfaces::SurfaceStatus::ConfigDrifted {
        file: tag.to_string(),
        diff: tag.to_string(),
      },
      surfaces::SurfaceStatus::ViolationsFound {
        message: tag.to_string(),
        diff: Some(tag.to_string()),
      },
      surfaces::SurfaceStatus::ExecutionError {
        message: tag.to_string(),
      },
    ]
  }

  /// The position `status` holds in [`every_status_by_precedence`].
  ///
  /// Exhaustive with no wildcard, so a new `surfaces::SurfaceStatus` does not compile
  /// until it is given a position here; placing it shifts every later arm,
  /// which fails `every_status_by_precedence_lists_each_variant_in_order`
  /// until the fixture lists it at that position too. A variant placed last
  /// shifts nothing, and stable Rust cannot count an enum's variants, so that
  /// one case still needs its fixture entry added by hand.
  fn variant_index(status: &surfaces::SurfaceStatus) -> usize {
    match status {
      surfaces::SurfaceStatus::Skipped { .. } => 0,
      surfaces::SurfaceStatus::Passed => 1,
      surfaces::SurfaceStatus::ConfigSynced { .. } => 2,
      surfaces::SurfaceStatus::ToolMissing { .. } => 3,
      surfaces::SurfaceStatus::ManualConfig { .. } => 4,
      surfaces::SurfaceStatus::ConfigDrifted { .. } => 5,
      surfaces::SurfaceStatus::ViolationsFound { .. } => 6,
      surfaces::SurfaceStatus::ExecutionError { .. } => 7,
    }
  }

  #[test]
  fn every_status_by_precedence_lists_each_variant_in_order() {
    let indices: Vec<usize> = every_status_by_precedence("x")
      .iter()
      .map(variant_index)
      .collect();
    assert_eq!(indices, (0..indices.len()).collect::<Vec<_>>());
  }

  fn combine_statuses(
    first: surfaces::SurfaceStatus,
    second: surfaces::SurfaceStatus,
  ) -> surfaces::SurfaceStatus {
    let result = |status| surfaces::SurfaceResult {
      surface_name: "test",
      status,
      duration: time::Duration::ZERO,
    };
    combine_pass_results(result(first), result(second)).status
  }

  #[test]
  fn combine_pass_results_higher_precedence_wins_in_either_order() {
    let ranked = every_status_by_precedence("x");
    for (i, lower) in ranked.iter().enumerate() {
      for higher in &ranked[i + 1..] {
        for (first, second) in [(lower, higher), (higher, lower)] {
          let combined = combine_statuses(first.clone(), second.clone());
          assert_eq!(
            format!("{combined:?}"),
            format!("{higher:?}"),
            "{first:?} + {second:?}"
          );
        }
      }
    }
  }

  #[test]
  fn combine_pass_results_same_variant_merges_or_keeps_first() {
    let firsts = every_status_by_precedence("a");
    let seconds = every_status_by_precedence("b");
    for (first, second) in firsts.into_iter().zip(seconds) {
      let expected = match &first {
        surfaces::SurfaceStatus::Skipped { .. } => {
          surfaces::SurfaceStatus::Skipped {
            reason: "a; b".to_string(),
          }
        }
        surfaces::SurfaceStatus::ViolationsFound { .. } => {
          surfaces::SurfaceStatus::ViolationsFound {
            message: "a\nb".to_string(),
            diff: Some("a\nb".to_string()),
          }
        }
        surfaces::SurfaceStatus::ExecutionError { .. } => {
          surfaces::SurfaceStatus::ExecutionError {
            message: "a\nb".to_string(),
          }
        }
        other => other.clone(),
      };
      let combined = combine_statuses(first, second);
      assert_eq!(format!("{combined:?}"), format!("{expected:?}"));
    }
  }

  #[test]
  fn exit_floor_agrees_with_is_success_and_rises_with_precedence() {
    let mut previous_floor = ExitStatus::Clean;
    for status in every_status_by_precedence("x") {
      let floor = exit_floor(&status.severity(), false);
      let result = surfaces::SurfaceResult {
        surface_name: "test",
        status,
        duration: time::Duration::ZERO,
      };
      assert_eq!(result.is_success(), floor.is_clean(), "{:?}", result.status);
      assert!(floor >= previous_floor, "{:?}", result.status);
      previous_floor = floor;
    }
  }

  #[test]
  fn runner_single_walk_polyglot_repo() {
    let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("tests/fixtures/polyglot_repo");

    // Single candidate filesystem walk
    let candidates = surfaces::glob::walk_candidate_files(&fixture, &[]);
    assert!(
      candidates.len() >= 7,
      "Expected at least 7 files in polyglot_repo, found {}",
      candidates.len()
    );

    // Filter in-memory for each surface
    let rust_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::rust::RustSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(rust_files.len(), 1);
    assert!(rust_files[0].ends_with("main.rs"));

    let py_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::python::PythonSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(py_files.len(), 1);
    assert!(py_files[0].ends_with("script.py"));

    let md_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::markdown::MarkdownSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(md_files.len(), 1);
    assert!(md_files[0].ends_with("README.md"));

    let yaml_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::yaml::YamlSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(yaml_files.len(), 1);
    assert!(yaml_files[0].ends_with("config.yaml"));

    let json_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::json::JsonSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(json_files.len(), 1);
    assert!(json_files[0].ends_with("data.json"));

    let typst_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::typst::TypstSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(typst_files.len(), 1);
    assert!(typst_files[0].ends_with("doc.typ"));

    let toml_files = surfaces::glob::filter_candidates_with_ext(
      &candidates,
      surfaces::LanguageSurface::file_extensions(
        &surfaces::lang::toml::TomlSurface,
      ),
      &[],
      &[],
    );
    assert_eq!(toml_files.len(), 1);
    assert!(toml_files[0].ends_with("Cargo.toml"));
  }

  #[test]
  fn execution_context_candidate_files_filtering() {
    let candidates = sync::Arc::new(vec![
      path::PathBuf::from("/ws/src/main.rs"),
      path::PathBuf::from("/ws/src/lib.rs"),
      path::PathBuf::from("/ws/src/ignored.rs"),
      path::PathBuf::from("/ws/script.py"),
    ]);

    let mut lang_config = config::ResolvedLangConfig::new("rust");
    lang_config.exclude = vec![path::PathBuf::from("ignored.rs")];

    let ctx = surfaces::ExecutionContext {
      root: sync::Arc::new(path::PathBuf::from("/ws")),
      paths: sync::Arc::new(Vec::new()),
      global_config: sync::Arc::new(config::ResolvedGlobalConfig::default()),
      lang_config,
      check_only: false,
      candidate_files: candidates,
    };

    let matched = ctx.matched_files(&["rs"]);
    assert_eq!(matched.len(), 2);
    assert!(matched.contains(&path::PathBuf::from("/ws/src/main.rs")));
    assert!(matched.contains(&path::PathBuf::from("/ws/src/lib.rs")));
    assert!(!matched.contains(&path::PathBuf::from("/ws/src/ignored.rs")));
    assert!(!matched.contains(&path::PathBuf::from("/ws/script.py")));
  }

  #[test]
  fn execution_context_staged_files_filtering() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();

    let src = root.join("src");
    let fixtures = root.join("fixtures");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&fixtures).unwrap();

    let main_rs = src.join("main.rs");
    let excluded_rs = src.join("generated.rs");
    let fixture_rs = fixtures.join("mock.rs");
    let py_file = root.join("script.py");

    std::fs::write(&main_rs, "fn main() {}\n").unwrap();
    std::fs::write(&excluded_rs, "fn gen() {}\n").unwrap();
    std::fs::write(&fixture_rs, "fn mock() {}\n").unwrap();
    std::fs::write(&py_file, "print('hi')\n").unwrap();

    let staged_paths =
      sync::Arc::new(vec![main_rs.clone(), excluded_rs, fixture_rs, py_file]);

    let mut lang_config = config::ResolvedLangConfig::new("rust");
    lang_config.exclude = vec![path::PathBuf::from("src/generated.rs")];

    let ctx = surfaces::ExecutionContext {
      root: sync::Arc::new(root.to_path_buf()),
      paths: sync::Arc::clone(&staged_paths),
      global_config: sync::Arc::new(config::ResolvedGlobalConfig::default()),
      lang_config,
      check_only: false,
      candidate_files: sync::Arc::new(surfaces::glob::expand_targets(
        root,
        &staged_paths,
      )),
    };

    let matched = ctx.matched_files(&["rs"]);
    assert_eq!(matched, vec![main_rs]);
  }

  /// A missing tool fails the run unless `--allow-missing` (#252, #163), and
  /// the flag never hides a real violation or tool error elsewhere.
  #[test]
  fn compute_exit_status_table() {
    let missing = || surfaces::SurfaceStatus::ToolMissing {
      binary: "t".into(),
      install_hint: String::new(),
    };
    let violation = || surfaces::SurfaceStatus::ViolationsFound {
      message: String::new(),
      diff: None,
    };
    let error = || surfaces::SurfaceStatus::ExecutionError {
      message: String::new(),
    };
    let skipped = || surfaces::SurfaceStatus::Skipped {
      reason: String::new(),
    };
    let passed = || surfaces::SurfaceStatus::Passed;
    let cases = [
      (vec![], false, ExitStatus::Clean),
      (vec![passed(), skipped()], false, ExitStatus::Clean),
      (vec![missing()], false, ExitStatus::Violations),
      (vec![missing()], true, ExitStatus::Clean),
      (vec![missing(), violation()], true, ExitStatus::Violations),
      (vec![missing(), error()], true, ExitStatus::Error),
      (vec![error(), violation()], false, ExitStatus::Error),
    ];
    for (statuses, allow_missing, expected) in cases {
      let results: Vec<_> = statuses
        .into_iter()
        .map(|status| surfaces::SurfaceResult {
          surface_name: "t",
          status,
          duration: time::Duration::ZERO,
        })
        .collect();
      assert_eq!(
        compute_exit_status(&results, allow_missing),
        expected,
        "{results:?} allow_missing={allow_missing}"
      );
    }
  }

  #[test]
  fn combine_pass_results_violations_over_tool_missing() {
    let lint_res = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::ToolMissing {
        binary: "markdownlint-cli2".to_string(),
        install_hint: "npm install -g markdownlint-cli2".to_string(),
      },
      duration: time::Duration::from_millis(10),
    };
    let fmt_res = surfaces::SurfaceResult {
      surface_name: "markdown",
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "unformatted".to_string(),
        diff: Some("diff".to_string()),
      },
      duration: time::Duration::from_millis(20),
    };

    let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
    assert!(matches!(
      combined.status,
      surfaces::SurfaceStatus::ViolationsFound { .. }
    ));
    assert_eq!(combined.duration, time::Duration::from_millis(30));
  }
}
