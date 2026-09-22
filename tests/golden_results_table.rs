//! Golden coverage for the results table — the one block of output every
//! `fml` run prints (issue #302).
//!
//! Every test here runs the **real binary** and asserts the rendered row for
//! a `SurfaceStatus`: its tag text, its surface-name cell, its detail text,
//! and — the coverage that was entirely absent before this file — the
//! `ui::table::Style` each of those three cells carries. Runs are made under
//! `FORCE_COLOR=1` with `COLORTERM=truecolor`, so `Palette::detect()`
//! resolves to [`Palette::truecolor`] and the SGR escapes are present in
//! stdout instead of being stripped. Expected escapes are derived from
//! `Palette::truecolor().style_sgr(style)` rather than hardcoded, so this
//! file pins the *semantic* style of each cell (which is what
//! `engine::runner::RowSpec` decides) and not the palette's colour values.
//!
//! The pre-existing `NO_COLOR` process tests in `tests/golden_output.rs` and
//! `tests/sync_reporting.rs` are deliberately untouched: this is an
//! additional colour-asserting path, not a replacement for the colour-free
//! one.
//!
//! All eight statuses are covered, each produced through the real
//! `Runner::run` path — no row is constructed by hand:
//!
//! | status | how it is produced |
//! |---|---|
//! | `ConfigSynced` | `fml sync` writing a native config that is not on disk |
//! | `Skipped` | the `typst` surface, which has no native config file |
//! | `Passed` | `fml sync --check` over a config that is already in sync |
//! | `ConfigDrifted` | a generated config edited, then `fml sync --check` |
//! | `ManualConfig` | a hand-written config with no formality header |
//! | `ToolMissing` | `fml fmt` with a `PATH` that has no `rustfmt`/`cargo` |
//! | `ExecutionError` | a `clang-format` shim that exits non-zero |
//! | `ViolationsFound` | a `typstyle` shim that exits non-zero |
//!
//! `ViolationsFound` renders three different detail texts since #119, all
//! three covered here: the bare `Violations found` above (a tool that says
//! nothing measurable), `N violations` (a tool that counts but claims
//! nothing about fixability), and `N violations, K auto-fixable` (a tool
//! that does both). The run summary's remaining-violations clause is
//! asserted alongside them, from the same runs, because its whole contract
//! is that it equals the sum of the rows.

use std::path::{Path, PathBuf};
use std::process::Command;

use fml::ui::table::{Palette, Style};

const SCHEMA_LINE: &str =
  "#:schema https://formality.dev/s1.1/formality.schema.json\n";

/// The opening SGR escape `Palette::truecolor()` renders `style` with.
///
/// Derived from the palette rather than written out, so a colour-value
/// change stays a colour-value change; only a change to which `Style` a cell
/// carries fails these tests.
fn open(style: Style) -> String {
  Palette::truecolor().style_sgr(style).0.to_string()
}

/// One styled cell of a rendered row: the SGR escape it opened with, and its
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StyledCell {
  sgr: String,
  text: String,
}

/// One rendered results-table row, reassembled from stdout.
#[derive(Debug, Clone)]
struct RenderedRow {
  tag: StyledCell,
  name: StyledCell,
  detail: StyledCell,
  /// The duration cell, with its value normalised to `<dur>` so nothing in
  /// this file is time-dependent. The style is kept as rendered.
  duration: StyledCell,
}

/// Splits one line of output into its styled runs, merging runs that are
/// adjacent and share a style.
///
/// The renderer emits each whitespace-separated token of a cell as its own
/// escape pair (`\x1b[2mAlready\x1b[0m\x1b[2m \x1b[0m\x1b[2min\x1b[0m…`), so
/// merging adjacent same-style runs rebuilds the whole cell. Padding
/// *between* cells is unstyled, which is what keeps two same-styled cells —
/// every cell of a `[SKIP]` row is `Dim` — from merging into one.
fn styled_runs(line: &str) -> Vec<StyledCell> {
  let mut runs: Vec<(Option<String>, String)> = Vec::new();
  let mut current: Option<String> = None;
  let mut buf = String::new();

  let flush = |runs: &mut Vec<(Option<String>, String)>,
               style: Option<String>,
               buf: &mut String| {
    if buf.is_empty() {
      return;
    }
    let text = std::mem::take(buf);
    match runs.last_mut() {
      Some((last_style, last_text)) if *last_style == style => {
        last_text.push_str(&text);
      }
      _ => runs.push((style, text)),
    }
  };

  let mut chars = line.chars().peekable();
  while let Some(c) = chars.next() {
    if c != '\u{1b}' {
      buf.push(c);
      continue;
    }
    let mut escape = String::from(c);
    for e in chars.by_ref() {
      escape.push(e);
      if e == 'm' {
        break;
      }
    }
    let next = if escape == "\u{1b}[0m" {
      None
    } else {
      Some(escape)
    };
    if next != current {
      flush(&mut runs, current.clone(), &mut buf);
      current = next;
    }
  }
  flush(&mut runs, current, &mut buf);

  runs
    .into_iter()
    .filter_map(|(sgr, text)| sgr.map(|sgr| StyledCell { sgr, text }))
    .collect()
}

/// Replaces a rendered duration (`122.60µs`, `2.05ms`, `0.00ns`, `1.23s`)
/// with `<dur>`.
///
/// Row assertions never read the value, but normalising it keeps a failure
/// message byte-stable between runs.
fn normalize_duration(text: &str) -> String {
  let is_duration = !text.is_empty()
    && text
      .trim_end_matches(['n', 'µ', 'm', 's'])
      .chars()
      .all(|c| c.is_ascii_digit() || c == '.')
    && text.ends_with('s')
    && text.chars().next().is_some_and(|c| c.is_ascii_digit());
  if is_duration {
    "<dur>".to_string()
  } else {
    text.to_string()
  }
}

/// Reassembles every results-table row from a run's stdout.
///
/// A row opens with a `[TAG]` cell; a detail too wide for its column wraps
/// onto following lines, whose styled runs are folded back into the detail
/// cell so assertions see the whole string. A rule line closes the table.
fn rendered_rows(stdout: &str) -> Vec<RenderedRow> {
  let mut rows: Vec<RenderedRow> = Vec::new();
  let mut in_table = false;

  for line in stdout.lines() {
    let runs = styled_runs(line);
    let is_rule = !runs.is_empty()
      && runs.iter().all(|r| r.text.chars().all(|c| c == '\u{2500}'));
    if is_rule || runs.is_empty() {
      in_table = false;
      continue;
    }

    let opens_row =
      runs[0].text.starts_with('[') && runs[0].text.ends_with(']');
    if opens_row {
      assert_eq!(
        runs.len(),
        4,
        "a results row should render 4 styled cells, got {runs:?}"
      );
      let mut duration = runs[3].clone();
      duration.text = normalize_duration(&duration.text);
      rows.push(RenderedRow {
        tag: runs[0].clone(),
        name: runs[1].clone(),
        detail: runs[2].clone(),
        duration,
      });
      in_table = true;
    } else if in_table {
      // A wrapped continuation of the row above: detail text only.
      let Some(row) = rows.last_mut() else {
        continue;
      };
      for run in runs {
        row.detail.text.push(' ');
        row.detail.text.push_str(&run.text);
      }
    }
  }

  rows
}

/// Asserts one status's whole rendered row: tag, surface-name cell and
/// detail, each with both its text and its style.
#[allow(clippy::too_many_arguments)]
fn assert_row(
  rows: &[RenderedRow],
  tag: &str,
  surface: &str,
  detail: &str,
  tag_style: Style,
  name_style: Style,
  detail_style: Style,
) {
  let row = rows
    .iter()
    .find(|r| r.tag.text == tag && r.name.text == surface)
    .unwrap_or_else(|| {
      panic!("no `{tag} {surface}` row in the rendered table:\n{rows:#?}")
    });

  assert_eq!(
    row.tag,
    StyledCell {
      sgr: open(tag_style),
      text: tag.to_string(),
    },
    "tag cell of the `{tag} {surface}` row"
  );
  assert_eq!(
    row.name,
    StyledCell {
      sgr: open(name_style),
      text: surface.to_string(),
    },
    "surface-name cell of the `{tag} {surface}` row"
  );
  assert_eq!(
    row.detail,
    StyledCell {
      sgr: open(detail_style),
      text: detail.to_string(),
    },
    "detail cell of the `{tag} {surface}` row"
  );
  assert_eq!(
    row.duration.sgr,
    open(Style::Dim),
    "duration cell of the `{tag} {surface}` row is always dim"
  );
  assert_eq!(row.duration.text, "<dur>");
}

/// Runs the real binary against `root` and returns its **styled** stdout.
///
/// `FORCE_COLOR` + `COLORTERM=truecolor` pin `Palette::detect()` to
/// [`Palette::truecolor`] regardless of whether the test harness's stdout is
/// a terminal; `NO_COLOR` is removed so an ambient one cannot strip the very
/// escapes under test.
fn run_fml(root: &Path, args: &[&str], path_env: Option<&Path>) -> String {
  let mut cmd = Command::new(env!("CARGO_BIN_EXE_fml"));
  cmd
    .args(args)
    .arg("--root")
    .arg(root)
    .env("FORCE_COLOR", "1")
    .env("COLORTERM", "truecolor")
    .env("TERM", "xterm-256color")
    .env_remove("NO_COLOR");
  if let Some(path) = path_env {
    cmd.env("PATH", path);
  }
  let out = cmd
    .output()
    .unwrap_or_else(|e| panic!("failed to run fml {args:?}: {e}"));
  String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A temp repo whose path is canonicalized, matching the existing golden
/// tests: on macOS a `TempDir` under `/var/...` is really `/private/var/...`.
fn temp_root(dir: &tempfile::TempDir) -> PathBuf {
  if cfg!(windows) {
    dir.path().to_path_buf()
  } else {
    std::fs::canonicalize(dir.path())
      .unwrap_or_else(|_| dir.path().to_path_buf())
  }
}

/// A tree with one surface that syncs a native config (`rust`) and one that
/// has none at all (`typst`). `fml sync` shells out to no external tool, so
/// every assertion below is hermetic.
fn sync_repo() -> tempfile::TempDir {
  let dir = tempfile::tempdir().expect("tempdir");
  std::fs::write(dir.path().join("formality.toml"), SCHEMA_LINE).unwrap();
  std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
  std::fs::write(dir.path().join("doc.typ"), "#let x = 1\n").unwrap();
  dir
}

#[test]
fn golden_sync_write_renders_config_synced_and_skipped_rows_with_styles() {
  let dir = sync_repo();
  let root = temp_root(&dir);
  let stdout = run_fml(&root, &["sync"], None);
  let rows = rendered_rows(&stdout);

  // `ConfigSynced`: a native config the surface had to create.
  assert_row(
    &rows,
    "[SYNC]",
    "rust",
    "Created .rustfmt.toml",
    Style::Ok,
    Style::Strong,
    Style::Info,
  );
  // `Skipped`: typst is configured entirely via CLI flags. This is the one
  // status whose surface-name cell is `Dim` rather than `Strong` — the sole
  // cell `RowSpec::name_style` exists to vary.
  assert_row(
    &rows,
    "[SKIP]",
    "typst",
    "No config file (settings applied via CLI flags)",
    Style::Dim,
    Style::Dim,
    Style::Dim,
  );
}

#[test]
fn golden_sync_check_renders_passed_drifted_and_manual_rows_with_styles() {
  let dir = sync_repo();
  let root = temp_root(&dir);
  // Generate the real configs first, so the `--check` pass below compares
  // against genuinely formality-generated files.
  let _ = run_fml(&root, &["sync"], None);

  // `Passed` ("Already in sync") is what `.editorconfig` reports on the
  // second pass; `ConfigDrifted` needs a generated file whose body no longer
  // matches. Keep the header so it stays formality-managed.
  let rustfmt = root.join(".rustfmt.toml");
  let generated = std::fs::read_to_string(&rustfmt).unwrap();
  let header: String = generated
    .lines()
    .take_while(|l| l.starts_with('#'))
    .map(|l| format!("{l}\n"))
    .collect();
  std::fs::write(&rustfmt, format!("{header}\ntab_spaces = 9\n")).unwrap();

  // `ManualConfig`: a config that exists but carries no formality header, so
  // sync must refuse to touch it.
  std::fs::write(root.join("thing.toml"), "a = 1\n").unwrap();
  std::fs::write(root.join("taplo.toml"), "# hand written\n").unwrap();

  let stdout = run_fml(&root, &["sync", "--check"], None);
  let rows = rendered_rows(&stdout);

  assert_row(
    &rows,
    "[DRIFT]",
    "rust",
    ".rustfmt.toml out of sync",
    Style::Warn,
    Style::Strong,
    Style::Warn,
  );
  assert_row(
    &rows,
    "[MANUAL]",
    "toml",
    "taplo.toml is manually managed",
    Style::Warn,
    Style::Strong,
    Style::Warn,
  );
  // `Passed` under a config-sync-only plan reads "Already in sync", not
  // "Clean / Formatted" (#130) — `passed_detail` is plan-dependent, and this
  // pins the sync spelling.
  assert_row(
    &rows,
    "[PASS]",
    "editorconfig",
    "Already in sync",
    Style::Ok,
    Style::Strong,
    Style::Dim,
  );
}

/// Writes an executable `#!/bin/sh` shim named `binary` into `dir` that
/// exits with `code`.
///
/// `which::which` (which `surfaces::tooling::resolve_binary_path` uses)
/// requires the executable bit for a `PATH` hit, so the mode is set
/// explicitly.
#[cfg(unix)]
fn write_shim(dir: &Path, binary: &str, code: i32) {
  use std::os::unix::fs::PermissionsExt;
  let path = dir.join(binary);
  std::fs::write(&path, format!("#!/bin/sh\nexit {code}\n")).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
    .unwrap();
}

/// The three statuses that need a real tool invocation, plus `Passed`'s
/// format-plan spelling, from one `fml fmt` run over a `PATH` containing
/// nothing but shims this test wrote.
///
/// Hermetic by construction: the run cannot see a real `rustfmt`,
/// `clang-format`, `typstyle` or `taplo`, so these statuses do not depend on
/// what the machine happens to have installed.
///
/// Unix-only: the shims are `#!/bin/sh` scripts. The `NO_COLOR` process
/// tests remain cross-platform; this is the colour-asserting addition.
#[cfg(unix)]
#[test]
fn golden_fmt_renders_missing_error_and_violation_rows_with_styles() {
  let shims = tempfile::tempdir().expect("tempdir");
  // `clang-format` cannot do its job -> `ExecutionError` (#151); `typstyle`
  // exits non-zero through the unclassified `run_tool_command` path ->
  // `ViolationsFound`; `taplo` succeeds -> `Passed`. No `rustfmt`/`cargo`
  // shim at all -> `ToolMissing`.
  write_shim(shims.path(), "clang-format", 1);
  write_shim(shims.path(), "typstyle", 1);
  write_shim(shims.path(), "taplo", 0);

  let dir = tempfile::tempdir().expect("tempdir");
  std::fs::write(dir.path().join("formality.toml"), SCHEMA_LINE).unwrap();
  std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
  std::fs::write(dir.path().join("main.cpp"), "int main() { return 0; }\n")
    .unwrap();
  std::fs::write(dir.path().join("doc.typ"), "#let x = 1\n").unwrap();
  std::fs::write(dir.path().join("thing.toml"), "a = 1\n").unwrap();
  let root = temp_root(&dir);

  let stdout = run_fml(&root, &["fmt"], Some(shims.path()));
  let rows = rendered_rows(&stdout);

  assert_row(
    &rows,
    "[MISS]",
    "rust",
    "Missing binary: cargo / rustfmt",
    Style::Warn,
    Style::Strong,
    Style::Warn,
  );
  assert_row(
    &rows,
    "[ERR]",
    "cpp",
    "Execution error",
    Style::Error,
    Style::Strong,
    Style::Error,
  );
  assert_row(
    &rows,
    "[FAIL]",
    "typst",
    "Violations found",
    Style::Error,
    Style::Strong,
    Style::Error,
  );
  // `Passed` again, under a format plan: the detail text is plan-dependent.
  assert_row(
    &rows,
    "[PASS]",
    "toml",
    "Clean / Formatted",
    Style::Ok,
    Style::Strong,
    Style::Dim,
  );
}

/// Writes an executable shim that prints `stdout` and exits with `code`.
///
/// [`write_shim`]'s silent form covers a tool that says nothing measurable;
/// this one lets a test hand the runner a tool's *real* summary lines
/// (captured from `ruff 0.15.8` and `markdownlint-cli2 v0.23.2`) without
/// depending on either being installed, or on the machine's version of
/// either still wording them the same way.
#[cfg(unix)]
fn write_speaking_shim(dir: &Path, binary: &str, stdout: &str, code: i32) {
  use std::os::unix::fs::PermissionsExt;
  let path = dir.join(binary);
  let script = stdout
    .lines()
    .map(|l| format!("echo '{}'\n", l.replace('\'', "'\\''")))
    .collect::<String>();
  std::fs::write(&path, format!("#!/bin/sh\n{script}exit {code}\n")).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
    .unwrap();
}

/// The run summary `fml` prints under the table, with its styling stripped
/// and its elapsed time normalised away.
fn summary_line(stdout: &str) -> String {
  let plain = fml::ui::table::strip_ansi_escapes(stdout);
  let line = plain
    .lines()
    .map(str::trim)
    .find(|l| {
      l.contains(" in ") && (l.contains("passed") || l.contains("failed"))
    })
    .unwrap_or_else(|| panic!("no run summary in:\n{plain}"))
    .to_string();
  match line.rfind(" in ") {
    Some(at) => line[..at].to_string(),
    None => line,
  }
}

/// `ruff check` over a file with two fixable and two unfixable findings,
/// verbatim from `ruff 0.15.8` (diagnostic bodies elided).
#[cfg(unix)]
const RUFF_MIXED_STDOUT: &str = "\
F401 [*] `os` imported but unused
B904 Within an `except` clause, raise exceptions with `raise ... from err`
Found 4 errors.
[*] 2 fixable with the `--fix` option.";

/// `markdownlint-cli2` over a file with two unfixable findings, verbatim
/// from v0.23.2 including the banner lines the surface filters out.
#[cfg(unix)]
const MARKDOWNLINT_STDOUT: &str = "\
markdownlint-cli2 v0.23.2 (markdownlint v0.41.1)
Linting: 1 file
Summary: 2 issues in 1 file
README.md:3:1 error MD033/no-inline-html Inline HTML [Element: p]
README.md:5 error MD036/no-emphasis-as-heading Emphasis used instead of a heading";

/// A tree with one `.py`, one `.md` and one `.toml`, plus the shims a run
/// over it needs. Returns both temp dirs so neither is dropped early.
#[cfg(unix)]
fn counted_repo(
  markdownlint_stdout: &str,
) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
  let shims = tempfile::tempdir().expect("tempdir");
  write_speaking_shim(shims.path(), "ruff", RUFF_MIXED_STDOUT, 1);
  write_speaking_shim(
    shims.path(),
    "markdownlint-cli2",
    markdownlint_stdout,
    1,
  );
  // `formality.toml` itself matches the toml surface, so the run would
  // otherwise report a missing `taplo` and change the summary's prefix.
  write_shim(shims.path(), "taplo", 0);

  let dir = tempfile::tempdir().expect("tempdir");
  std::fs::write(dir.path().join("formality.toml"), SCHEMA_LINE).unwrap();
  std::fs::write(dir.path().join("app.py"), "import os\n").unwrap();
  std::fs::write(dir.path().join("README.md"), "# T\n").unwrap();
  let root = temp_root(&dir);
  (dir, shims, root)
}

/// The two measured `[FAIL]` spellings, and the summary clause that is the
/// sum of exactly those two rows (#119).
///
/// `fml lint` is the plan used because it is one pass per surface: what each
/// shim prints is what the row reports, with no format pass or post-format
/// recheck in between to reason about.
#[cfg(unix)]
#[test]
fn golden_lint_renders_counted_violation_rows_and_summary() {
  let (_dir, shims, root) = counted_repo(MARKDOWNLINT_STDOUT);
  let stdout = run_fml(&root, &["lint"], Some(shims.path()));
  let rows = rendered_rows(&stdout);

  // ruff answers both questions itself: `Found 4 errors.` and its own
  // ``[*] 2 fixable`` hint.
  assert_row(
    &rows,
    "[FAIL]",
    "python",
    "4 violations, 2 auto-fixable",
    Style::Error,
    Style::Strong,
    Style::Error,
  );
  // markdownlint-cli2 counts but marks no violation fixable, and this plan
  // ran no fixer — so the count is reported with no fixability claim rather
  // than a guessed zero.
  assert_row(
    &rows,
    "[FAIL]",
    "markdown",
    "2 violations",
    Style::Error,
    Style::Strong,
    Style::Error,
  );

  // 4 + 2, straight off the rows above. No fixability figure: markdown
  // withheld its half, and half a claim reads as a whole one.
  assert_eq!(
    summary_line(&stdout),
    "1 passed, 2 failed (6 violations remaining)"
  );
}

/// The wording the issue is actually about: everything that is left needs a
/// human (#119).
#[cfg(unix)]
#[test]
fn golden_lint_says_in_words_when_nothing_left_is_auto_fixable() {
  let markdownlint = MARKDOWNLINT_STDOUT.replace(
    "Linting: 1 file",
    "Linting: 1 file\nAttempted: 3 fixes in 1 file",
  );
  let (_dir, shims, root) = counted_repo(&markdownlint);
  // A `ruff` that fixed what it could and says so: 2 remaining, and no
  // ``[*]`` hint beside the `Found` line, which is ruff stating that none of
  // the two are fixable.
  write_speaking_shim(
    shims.path(),
    "ruff",
    "B904 Within an `except` clause, raise exceptions with `raise ... from err`\n\
     Found 4 errors (2 fixed, 2 remaining).",
    1,
  );

  let stdout = run_fml(&root, &["lint"], Some(shims.path()));
  let rows = rendered_rows(&stdout);

  // The count is the *remaining* 2, never the leading 4 — those no longer
  // exist.
  assert_row(
    &rows,
    "[FAIL]",
    "python",
    "2 violations, 0 auto-fixable",
    Style::Error,
    Style::Strong,
    Style::Error,
  );
  // markdownlint's `Attempted:` line is its statement that it ran its fixer,
  // so what it still reports is what it could not fix.
  assert_row(
    &rows,
    "[FAIL]",
    "markdown",
    "2 violations, 0 auto-fixable",
    Style::Error,
    Style::Strong,
    Style::Error,
  );

  assert_eq!(
    summary_line(&stdout),
    "1 passed, 2 failed (4 violations remaining, none auto-fixable \u{2014} manual edits needed)"
  );
}

/// A failing surface `fml` cannot measure suppresses the summary clause
/// outright, and keeps its own row's wording (#119).
///
/// A total that silently omits a surface is worse than no total: nothing in
/// the line would say it is partial.
#[cfg(unix)]
#[test]
fn golden_lint_suppresses_the_summary_clause_when_a_surface_is_uncounted() {
  let (_dir, shims, root) = counted_repo(MARKDOWNLINT_STDOUT);
  // `taplo` exits non-zero saying nothing a count can be read out of.
  write_shim(shims.path(), "taplo", 1);
  std::fs::write(root.join("thing.toml"), "a = 1\n").unwrap();

  let stdout = run_fml(&root, &["lint"], Some(shims.path()));
  let rows = rendered_rows(&stdout);

  assert_row(
    &rows,
    "[FAIL]",
    "toml",
    "Violations found",
    Style::Error,
    Style::Strong,
    Style::Error,
  );
  assert_eq!(summary_line(&stdout), "3 failed");
}
