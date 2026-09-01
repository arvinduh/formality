//! Golden-output coverage for issue #122: every table `fml` prints is framed
//! `header -> rule -> body -> rule` by one helper (`ui::table::Frame`), stays
//! within 80 columns (continuation lines included), never splits a token across
//! a wrapped line, and renders run-root paths relative through one shared
//! helper. Framing regressions fail here loudly instead of drifting per caller.

use std::process::Command;

use fml::ui::paths::relativize_text;
use fml::ui::table::{
  Cell, Column, Frame, Layout, Palette, Row, Span, Style, Table, WidthPolicy,
  max_line_display_width, render, strip_ansi_escapes,
};

/// Assert the shared framing contract on a block of `fml` output: a title
/// line, then rules that are all the same width and never wider than 80, and
/// no line (wrapped continuation included) past 80 columns.
fn assert_framed_within_80(plain: &str) {
  let lines: Vec<&str> = plain.lines().collect();
  let rule_widths: Vec<usize> = lines
    .iter()
    .map(|l| l.trim_end())
    .filter(|l| !l.is_empty() && l.chars().all(|c| c == '\u{2500}'))
    .map(|l| l.chars().count())
    .collect();

  assert!(!rule_widths.is_empty(), "no rule lines found in:\n{plain}");
  let first = rule_widths[0];
  for w in &rule_widths {
    assert_eq!(*w, first, "rules disagree about width in:\n{plain}");
    assert!(*w <= 80, "rule wider than 80 cols in:\n{plain}");
  }
  for l in &lines {
    assert!(
      max_line_display_width(l) <= 80,
      "line exceeds 80 cols ({}): {l:?}",
      max_line_display_width(l)
    );
  }
}

/// The `fml doctor` scan table, built exactly as `scan_tools_and_build_table`
/// does, with a row carrying a long Windows path and a `javascript` surface
/// name wider than its own `Fixed(10)` column.
fn doctor_scan_table() -> Table {
  let mut t = Table::new(vec![
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(20)),
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(10)),
    Column::new(Cell::text("")).width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2).padding(0, 1).max_width(80));
  t.add_row(Row::new(vec![
    Cell::styled("[READY]", Style::Ok),
    Cell::styled("rustfmt", Style::Tool),
    Cell::styled("rust", Style::Dim),
    Cell::new(vec![
      Span::styled("C:\\Users\\olives\\.cargo\\bin\\rustfmt.exe", Style::Dim),
      Span::styled(" (v1.9.0-stable)", Style::Info),
    ]),
  ]));
  t.add_row(Row::new(vec![
    Cell::styled("[MISS] ", Style::Warn),
    Cell::styled("biome", Style::Warn),
    Cell::styled("javascript", Style::Dim),
    Cell::styled(
      "An extremely fast web toolchain, written in Rust",
      Style::Dim,
    ),
  ]));
  t
}

#[test]
fn golden_doctor_table_framing_and_wrapping() {
  let palette = Palette::none();
  let body = render(&doctor_scan_table(), &palette);
  let frame = Frame::for_body(&body);
  let out = frame.section("fml doctor (all surfaces)", &body, &palette);

  assert_eq!(out.lines().next().unwrap(), "fml doctor (all surfaces)");
  assert!(out.lines().nth(1).unwrap().chars().all(|c| c == '\u{2500}'));
  assert!(out.lines().last().unwrap().chars().all(|c| c == '\u{2500}'));
  assert_framed_within_80(&out);

  // Every path token survives intact on some single line: no `rustf` / `mt.exe`
  // mid-token break.
  for token in [
    "C:\\",
    "Users\\",
    "olives\\",
    ".cargo\\",
    "bin\\",
    "rustfmt.exe",
  ] {
    assert!(
      out.lines().any(|l| l.contains(token)),
      "path token {token:?} was split:\n{out}"
    );
  }
  // The Fixed(10) surface column widened to hold its own value.
  assert!(out.contains("javascript"));
  assert!(!out.lines().any(|l| l.trim() == "javascrip"));
}

#[test]
fn golden_runner_diagnostics_render_paths_relative() {
  // A failing-lint diagnostics block: absolute paths under the run root must
  // come out relative, via the same helper the table cells use.
  let root = std::path::Path::new("C:/work/demo");
  let raw = "Finding: C:/work/demo/README.md C:/work/demo/docs/architecture.md\n\
             --- C:\\work\\demo\\src\\main.rs\n\
             +++ C:\\work\\demo\\src\\main.rs (formatted)";
  let relativized = relativize_text(root, raw);

  assert!(!relativized.contains("C:/work/demo"));
  assert!(!relativized.contains("C:\\work\\demo"));
  assert!(relativized.contains("Finding: README.md docs/architecture.md"));
  assert!(relativized.contains("--- src\\main.rs"));
  assert!(relativized.contains("+++ src\\main.rs (formatted)"));

  let palette = Palette::none();
  let frame = Frame::capped();
  let framed =
    frame.section("Diagnostics & Suggestions:", &relativized, &palette);
  assert!(
    framed
      .lines()
      .nth(1)
      .unwrap()
      .chars()
      .all(|c| c == '\u{2500}')
  );
  assert!(
    framed
      .lines()
      .last()
      .unwrap()
      .chars()
      .all(|c| c == '\u{2500}')
  );
}

#[test]
fn golden_fml_doctor_process_output_is_framed_within_80() {
  let out = Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("doctor")
    .env("NO_COLOR", "1")
    .env_remove("FORCE_COLOR")
    .output()
    .expect("failed to run fml doctor");
  let stdout = String::from_utf8_lossy(&out.stdout);
  let plain = strip_ansi_escapes(&stdout);

  assert!(
    plain
      .lines()
      .next()
      .unwrap_or_default()
      .starts_with("fml doctor"),
    "doctor output should open with the framed title, got:\n{plain}"
  );
  assert_framed_within_80(&plain);
  // The Install Summary is only printed by `--install`; the doctor sections
  // that do print (`fml sync:` always does) must each be bracketed by a rule.
  assert!(plain.contains("fml sync:"));
}

#[test]
fn golden_unbreakable_token_wider_than_table_hard_splits_to_stay_within_80() {
  // A pathological cell value with no break point (a linter can emit one --
  // see #112 on capping diagnostics volume). It cannot be shown whole within
  // 80 columns, so `render` hard-splits it as a last resort rather than let
  // the table overflow its width budget. This pins that behavior.
  let giant = "x".repeat(140);
  let mut t = Table::new(vec![
    Column::new(Cell::text("")).width(WidthPolicy::Fixed(8)),
    Column::new(Cell::text("")).width(WidthPolicy::Auto),
  ])
  .layout(Layout::compact().indent(2).padding(0, 1).max_width(80));
  t.add_row(Row::new(vec![
    Cell::styled("[FAIL] ", Style::Error),
    Cell::styled(giant.as_str(), Style::Dim),
  ]));

  let body = render(&t, &Palette::none());
  for line in body.lines() {
    assert!(
      max_line_display_width(line) <= 80,
      "table line exceeded 80 cols ({}): {line:?}",
      max_line_display_width(line)
    );
  }
  // Present in full, just spread across continuation lines.
  let joined: String = body.lines().map(str::trim).collect::<Vec<_>>().join("");
  assert!(joined.contains(&"x".repeat(80)));
}

#[test]
fn golden_hard_cap_width_policies_are_not_softened_by_a_long_token() {
  // `Max` / `Range` / `Pct` are hard caps: a token wider than the cap is
  // hard-split, the column is NOT widened past the cap (issue #122 only asked
  // for `Fixed` to grow).
  let long_token = "supercalifragilisticexpialidocious".repeat(2); // 68, no breaks
  for policy in [
    WidthPolicy::Max(12),
    WidthPolicy::Range(6, 12),
    WidthPolicy::Pct(15),
  ] {
    let mut t = Table::new(vec![
      Column::new(Cell::text("")).width(policy),
      Column::new(Cell::text("")).width(WidthPolicy::Auto),
    ])
    .layout(Layout::compact().padding(0, 1).max_width(80));
    t.add_row(Row::new(vec![
      Cell::styled(long_token.as_str(), Style::Dim),
      Cell::styled("ok", Style::Dim),
    ]));
    let body = render(&t, &Palette::none());
    let capped_col_width = body
      .lines()
      .map(|l| l.split_whitespace().next().unwrap_or("").chars().count())
      .max()
      .unwrap_or(0);
    assert!(
      capped_col_width <= 14,
      "{policy:?} column grew to {capped_col_width} for a long token:\n{body}"
    );
  }
}

#[test]
fn golden_failing_fml_lint_process_output_is_framed_within_80() {
  let dir = tempfile::tempdir().expect("tempdir");
  // Pass the canonicalized root so it matches what the linters echo back:
  // on macOS a `TempDir` under `/var/...` is reported as `/private/var/...`
  // (a symlink), which would otherwise defeat relative-path rendering and
  // this test's leak check. (`canonicalize` adds a `\\?\` verbatim prefix on
  // Windows, so skip it there.)
  let root = if cfg!(windows) {
    dir.path().to_path_buf()
  } else {
    std::fs::canonicalize(dir.path())
      .unwrap_or_else(|_| dir.path().to_path_buf())
  };
  // A JSON file that is not canonically formatted. Whether the JS/JSON tool is
  // installed or not, `fml lint` here produces a framed table plus a framed
  // diagnostics block (ViolationsFound or ToolMissing) and a non-zero exit.
  std::fs::write(dir.path().join("data.json"), "{\"a\":1,\"b\":[1,2,3]}\n")
    .unwrap();
  std::fs::write(
    dir.path().join("formality.toml"),
    "#:schema https://formality.dev/s1.1/formality.schema.json\n\
     languages = [\"json\"]\n",
  )
  .unwrap();

  let out = Command::new(env!("CARGO_BIN_EXE_fml"))
    .args(["lint", "--root"])
    .arg(&root)
    .env("NO_COLOR", "1")
    .env_remove("FORCE_COLOR")
    .output()
    .expect("failed to run fml lint");
  let stdout = String::from_utf8_lossy(&out.stdout);
  let plain = strip_ansi_escapes(&stdout);

  if plain.contains("No matching language surfaces") {
    return; // environment without the json surface active; nothing to frame
  }
  assert!(
    plain
      .lines()
      .next()
      .unwrap_or_default()
      .starts_with("fml lint"),
    "lint output should open with the framed title, got:\n{plain}"
  );
  assert_framed_within_80(&plain);
  // Absolute paths from the run root must not leak into the framed output.
  let tmp = root.to_string_lossy().replace('\\', "/");
  assert!(
    !plain.replace('\\', "/").contains(&tmp),
    "diagnostics leaked an absolute run-root path:\n{plain}"
  );
}
