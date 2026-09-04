//! End-to-end coverage for what `fml sync` *tells the user it did* (issue
//! #130). These assertions run the real binary and read its stdout, because
//! all three defects the issue reports are defects of the report rather than
//! of the files on disk: a header count that disagreed with the rows below
//! it, a config file written but never named, and a shared `.prettierrc.json`
//! whose "who created it" credit depended on which rayon worker won a race.
//!
//! `fml sync` shells out to no external tool, so these tests are hermetic —
//! they pass whether or not prettier/markdownlint/yamllint are installed.

use fml::ui::table::strip_ansi_escapes;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const SCHEMA_LINE: &str =
  "#:schema https://formality.dev/s1.1/formality.schema.json\n";

/// A polyglot tree whose surfaces overlap on `.prettierrc.json` (json,
/// markdown and yaml all format via prettier) and which also contains a
/// surface that syncs two native config files of its own (cpp).
fn polyglot_repo() -> tempfile::TempDir {
  let dir = tempfile::tempdir().expect("tempdir");
  let root = dir.path();
  std::fs::write(root.join("formality.toml"), SCHEMA_LINE).unwrap();
  std::fs::write(root.join("README.md"), "# Title\n\nBody text.\n").unwrap();
  std::fs::write(root.join("data.json"), "{ \"a\": 1 }\n").unwrap();
  std::fs::write(root.join("config.yaml"), "a: 1\n").unwrap();
  std::fs::write(root.join("main.cpp"), "int main() { return 0; }\n").unwrap();
  dir
}

/// Runs `fml sync` against `root` and returns its ANSI-stripped stdout.
fn run_sync(root: &Path, extra: &[&str]) -> String {
  let out = Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("sync")
    .arg("--root")
    .arg(root)
    .args(extra)
    .env("NO_COLOR", "1")
    .env_remove("FORCE_COLOR")
    .output()
    .expect("failed to run fml sync");
  strip_ansi_escapes(&String::from_utf8_lossy(&out.stdout))
}

/// The `(N surfaces)` count from the framed header line.
fn header_count(plain: &str) -> usize {
  let line = plain
    .lines()
    .find(|l| l.trim_start().starts_with("fml sync"))
    .unwrap_or_else(|| panic!("no `fml sync` header line in:\n{plain}"));
  let inner = line
    .rsplit_once('(')
    .and_then(|(_, rest)| rest.split_once(' '))
    .map(|(n, _)| n)
    .unwrap_or_else(|| panic!("unparseable header line {line:?}"));
  inner
    .parse()
    .unwrap_or_else(|e| panic!("unparseable count in {line:?}: {e}"))
}

/// Every status row the table rendered. Wrapped continuation lines do not
/// open with a `[STATUS]` token, so they are not miscounted as rows.
fn status_rows(plain: &str) -> Vec<&str> {
  plain
    .lines()
    .map(str::trim_start)
    .filter(|l| {
      [
        "[PASS]", "[SYNC]", "[DRIFT]", "[MANUAL]", "[FAIL]", "[MISS]", "[ERR]",
        "[SKIP]",
      ]
      .iter()
      .any(|tag| l.starts_with(tag))
    })
    .collect()
}

#[test]
fn sync_header_count_matches_the_rows_it_renders() {
  // Defect 1 of #130: the header rendered `surfaces.len()`, but `fml sync`
  // appends shared-config rows after the per-surface fan-out. `fml sync -l
  // markdown` therefore printed `1 surface` above two rows, with a footer
  // reading `2 passed` — a deterministic off-by-one on every single run.
  let dir = tempfile::tempdir().expect("tempdir");
  std::fs::write(dir.path().join("formality.toml"), SCHEMA_LINE).unwrap();
  std::fs::write(dir.path().join("README.md"), "# Title\n").unwrap();

  let plain = run_sync(dir.path(), &["--lang", "markdown"]);
  assert_eq!(
    header_count(&plain),
    status_rows(&plain).len(),
    "header count disagrees with rendered rows:\n{plain}"
  );
}

#[test]
fn sync_header_count_matches_the_rows_it_renders_on_a_polyglot_tree() {
  let dir = polyglot_repo();
  let plain = run_sync(dir.path(), &[]);
  assert_eq!(
    header_count(&plain),
    status_rows(&plain).len(),
    "header count disagrees with rendered rows:\n{plain}"
  );
}

/// Every file `fml sync` left in `root`, excluding the fixture sources it
/// was pointed at.
fn generated_config_files(root: &Path) -> BTreeSet<String> {
  let fixtures: BTreeSet<&str> = [
    "formality.toml",
    "README.md",
    "data.json",
    "config.yaml",
    "main.cpp",
  ]
  .into_iter()
  .collect();
  std::fs::read_dir(root)
    .expect("read_dir")
    .filter_map(Result::ok)
    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
    .map(|e| e.file_name().to_string_lossy().into_owned())
    .filter(|n| !fixtures.contains(n.as_str()))
    .collect()
}

#[test]
fn sync_names_every_file_it_writes() {
  // Defect 2 of #130: `MarkdownSurface::sync_config` synced
  // `.markdownlint.json` and then `.prettierrc.json` but returned only the
  // second result, so `.markdownlint.json` appeared on disk having never been
  // named in the output. A user auditing what `fml` put in their repo could
  // not find it. This is the single regression test the issue asked for: it
  // compares the report against the directory listing, so any future silently
  // written config file fails here too.
  let dir = polyglot_repo();
  let plain = run_sync(dir.path(), &[]);

  let on_disk = generated_config_files(dir.path());
  assert!(
    !on_disk.is_empty(),
    "fixture produced no config files at all:\n{plain}"
  );
  for file in &on_disk {
    assert!(
      plain.contains(file.as_str()),
      "`{file}` was written to disk but never named in the output:\n{plain}"
    );
  }
}

#[test]
fn sync_no_op_reads_as_already_in_sync() {
  // Issue #130: a second `fml sync` over an already-synced tree rewrites
  // nothing, and `SurfaceStatus::Passed` rendered `Clean / Formatted` — the
  // vocabulary of the format pass. Nothing was formatted; the config files
  // simply already matched formality.toml.
  let dir = polyglot_repo();
  let first = run_sync(dir.path(), &[]);
  assert!(
    first.contains("Created "),
    "first sync should have created files:
{first}"
  );

  let second = run_sync(dir.path(), &[]);
  assert!(
    second.contains("Already in sync"),
    "a sync no-op should read as already-in-sync:
{second}"
  );
  assert!(
    !second.contains("Clean / Formatted"),
    "`fml sync` must not borrow the format pass's vocabulary:
{second}"
  );
}

/// Blanks the elapsed-time tokens, which legitimately differ run to run, so
/// everything else in the report can be compared byte for byte.
fn scrub_timings(plain: &str) -> String {
  plain
    .lines()
    .map(|line| {
      line
        .split_whitespace()
        .map(|tok| {
          let is_duration = tok.starts_with(|c: char| c.is_ascii_digit())
            && (tok.ends_with("ms")
              || tok.ends_with("µs")
              || tok.ends_with("ns")
              || tok.ends_with('s'));
          if is_duration { "<t>" } else { tok }
        })
        .collect::<Vec<_>>()
        .join(" ")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn prettierrc_has_exactly_one_writer() {
  // Defect 3 of #130: json, markdown and yaml all called
  // `sync_prettier_config` from their own `sync_config`, which the runner
  // invokes under `surfaces.par_iter()` — three threads running an unlocked
  // read-compare-write against one path. Whichever won reported `Created
  // .prettierrc.json` and the others reported a no-op, so the credited
  // surface varied run to run. The write now happens once, outside the
  // fan-out, in a pass that names itself `prettier`.
  let dir = polyglot_repo();
  let plain = run_sync(dir.path(), &[]);

  // Only rows that claim to have *written* the file count as writers; the
  // json surface still names it in a `[SKIP]` row to say it shares it.
  let claiming_rows: Vec<&str> = status_rows(&plain)
    .into_iter()
    .filter(|r| {
      r.contains(".prettierrc.json")
        && (r.starts_with("[SYNC]")
          || r.starts_with("[DRIFT]")
          || r.starts_with("[MANUAL]"))
    })
    .collect();
  assert_eq!(
    claiming_rows.len(),
    1,
    "`.prettierrc.json` must have exactly one writer, got {claiming_rows:?} in:\n{plain}"
  );
  assert!(
    claiming_rows[0].contains("prettier"),
    "the shared pass should own the row: {:?}",
    claiming_rows[0]
  );
  assert!(dir.path().join(".prettierrc.json").is_file());
}

#[test]
fn repeated_sync_on_a_polyglot_tree_is_byte_identical() {
  // The regression test for the race itself: with three surfaces racing on
  // one path, which of them reported `Created .prettierrc.json` (and which
  // reported a no-op) was decided by whichever rayon worker won, so two
  // consecutive runs over a settled tree could differ. Timings aside, they
  // must now be identical.
  let dir = polyglot_repo();
  let _first = run_sync(dir.path(), &[]);
  let second = scrub_timings(&run_sync(dir.path(), &[]));
  let third = scrub_timings(&run_sync(dir.path(), &[]));
  let fourth = scrub_timings(&run_sync(dir.path(), &[]));

  assert_eq!(second, third, "run 2 and run 3 disagree");
  assert_eq!(third, fourth, "run 3 and run 4 disagree");
}

#[test]
fn sync_check_agrees_with_itself_on_a_settled_tree() {
  // `fml sync --check` is a `.pre-commit-hooks.yaml` entry point, and the
  // same race made it capable of disagreeing with itself between runs.
  let dir = polyglot_repo();
  let _ = run_sync(dir.path(), &[]);

  let a = scrub_timings(&run_sync(dir.path(), &["--check"]));
  let b = scrub_timings(&run_sync(dir.path(), &["--check"]));
  assert_eq!(a, b, "`fml sync --check` disagreed with itself");
  assert!(
    !a.contains("[DRIFT]"),
    "a freshly synced tree must not report drift:\n{a}"
  );
}
