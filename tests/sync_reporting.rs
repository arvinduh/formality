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
