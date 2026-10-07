//! `fml fmt` and `fml lint`: per-surface lifecycles, path targeting,
//! `--staged`/`--changed` filtering, language filters,

use std::fs;
use std::path;

use fml::config;
use fml::engine::runner;
use fml::surfaces;

use crate::common;

#[test]
fn and_lint_lifecycle() {
  let temp = common::temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"lifecycle_test\"\nversion = \"0.1.0\"\nedition = \
       \"2024\"\n",
    ),
    ("src/main.rs", "fn main() {\nprintln!(\"hello\");\n}\n"),
  ]);
  let root = temp.path();

  // 1. Format the codebase
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(false, &["rust"])),
    runner::ExitStatus::Clean
  );

  // 2. Check formatting (should be clean now)
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["rust"])),
    runner::ExitStatus::Clean
  );
}

#[test]
fn targeted_file_and_dir_formatting() {
  let temp = common::temp_repo(&[
    (
      "nested/target.rs",
      "fn target() {\nprintln!(\"target\");\n}\n",
    ),
    (
      "untouched.rs",
      "fn untouched() {\nprintln!(\"untouched\");\n}\n",
    ),
  ]);
  let root = temp.path();
  let target_file = root.join("nested/target.rs");
  let sub = root.join("nested");

  // Format only target_file
  let fmt_single = common::Command::Fmt {
    check: false,
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    allow_missing: false,
    paths: vec![target_file],
  };
  assert_eq!(
    common::run_cli(root, &fmt_single),
    runner::ExitStatus::Clean
  );

  // Format nested directory
  let fmt_dir = common::Command::Fmt {
    check: false,
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    allow_missing: false,
    paths: vec![sub],
  };
  assert_eq!(common::run_cli(root, &fmt_dir), runner::ExitStatus::Clean);
}

#[test]
fn ignore_languages_filtering() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let poly_root = manifest_dir.join("tests/fixtures/polyglot_repo");

  let config_str = r#"
    [global]
    ignore_languages = ["markdown", "yaml", "json"]
  "#;
  let config = config::FormalityConfig::parse_str(
    config_str,
    path::Path::new("formality.toml"),
  )
  .unwrap();

  let detected = surfaces::registry::detect_surfaces_smart(&poly_root, &config);
  let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

  assert!(names.contains(&"rust"));
  assert!(names.contains(&"python"));
  assert!(!names.contains(&"markdown"));
  assert!(!names.contains(&"yaml"));
  assert!(!names.contains(&"json"));
}

#[test]
fn autodetect_all_workspace_surfaces_by_default() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let poly_root = manifest_dir.join("tests/fixtures/polyglot_repo");

  // Default config without explicit languages list — auto-detect mode
  let config = config::FormalityConfig::with_defaults();
  assert_eq!(config.resolve_global().languages, None);

  let detected = surfaces::registry::detect_surfaces_smart(&poly_root, &config);
  let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

  assert!(names.contains(&"rust"));
  assert!(names.contains(&"python"));
  assert!(names.contains(&"markdown"));
  assert!(names.contains(&"toml"));
  assert!(names.contains(&"yaml"));
  assert!(names.contains(&"json"));
}

#[test]
fn python_import_sorting_lifecycle() {
  if which::which("ruff").is_err() {
    eprintln!(
      "Skipping python_import_sorting_lifecycle: ruff not installed \
       in PATH"
    );
    return;
  }

  let temp = common::temp_repo(&[
    ("pyproject.toml", "[project]\nname = \"test\"\n"),
    ("main.py", "import sys\nimport os\n\ndef greet():\n  pass\n"),
  ]);
  let root = temp.path();
  let py_file = root.join("main.py");

  // 1. fmt --check should fail because imports are not sorted
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["python"])),
    runner::ExitStatus::Violations
  );

  // 2. fmt (write mode) should sort imports
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(false, &["python"])),
    runner::ExitStatus::Clean
  );

  let formatted = fs::read_to_string(&py_file).unwrap();
  let os_pos = formatted.find("import os").expect("import os present");
  let sys_pos = formatted.find("import sys").expect("import sys present");
  assert!(
    os_pos < sys_pos,
    "import os must precede import sys after sorting"
  );

  // 3. fmt --check should now pass
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["python"])),
    runner::ExitStatus::Clean
  );
}

#[test]
fn markdown_prettier_extra_args_converge() {
  // A `prettier` extra arg that overrides an inline-config flag must win on
  // both the write and the `--check` path; prettier honours the last copy of
  // a flag, so differing argv orders left `--check` failing forever.
  if which::which("prettier").is_err() {
    eprintln!("Skipping: prettier not installed in PATH");
    return;
  }

  let temp = common::temp_repo(&[
    (
      "formality.toml",
      "[lang.markdown]\nline_length = 80\nprose_wrap = \"always\"\n\n\
       [lang.markdown.extra_args]\nprettier = [\"--print-width\", \"40\"]\n",
    ),
    (
      "a.md",
      "# Title\n\nThis is a fairly long sentence of prose that should be \
       wrapped by prettier at some width or other.\n",
    ),
  ]);
  let root = temp.path();

  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(false, &["markdown"])),
    runner::ExitStatus::Clean
  );
  let formatted = fs::read_to_string(root.join("a.md")).unwrap();
  assert!(
    formatted.lines().all(|l| l.len() <= 40),
    "the user's --print-width must win on the write path:\n{formatted}"
  );
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["markdown"])),
    runner::ExitStatus::Clean
  );
}

#[test]
fn rust_import_reordering_lifecycle() {
  let temp = common::temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"reorder_test\"\nversion = \"0.1.0\"\nedition = \
       \"2024\"\n",
    ),
    (
      "src/main.rs",
      "use std::time::Instant;\nuse std::collections::HashMap;\nuse \
       std::path::Path;\n\nfn main() {\n  let _ = (HashMap::<u32, u32>::new(), \
       Path::new(\"/\"), Instant::now());\n}\n",
    ),
  ]);
  let root = temp.path();
  let main_rs = root.join("src/main.rs");

  // 1. fmt --check should report formatting issues due to unsorted imports
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["rust"])),
    runner::ExitStatus::Violations
  );

  // 2. fmt (write mode) should reorder imports
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(false, &["rust"])),
    runner::ExitStatus::Clean
  );

  let formatted = fs::read_to_string(&main_rs).unwrap();
  let hashmap_pos = formatted
    .find("use std::collections::HashMap;")
    .expect("HashMap import present");
  let path_pos = formatted
    .find("use std::path::Path;")
    .expect("Path import present");
  let instant_pos = formatted
    .find("use std::time::Instant;")
    .expect("Instant import present");
  assert!(hashmap_pos < path_pos, "HashMap must precede Path");
  assert!(path_pos < instant_pos, "Path must precede Instant");

  // 3. fmt --check should now pass
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["rust"])),
    runner::ExitStatus::Clean
  );
}

#[test]
fn staged_and_changed_with_explicit_paths_filtering() {
  let temp = common::temp_repo(&[
    ("a.toml", "[package]\nname = \"a\"\n"),
    ("b.toml", "[package]\nname = \"b\"\n"),
  ]);
  let root = temp.path();

  if !common::init_git_repo(root) {
    return;
  }

  let file_a = root.join("a.toml");
  let file_b = root.join("b.toml");

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["commit", "-m", "initial"])
    .current_dir(root)
    .output();

  // Modify and stage both
  fs::write(&file_a, "[package]\n   name =   \"a_mod\"\n").unwrap();
  fs::write(&file_b, "[package]\n   name =   \"b_mod\"\n").unwrap();
  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(root)
    .output();

  // fmt only a.toml
  let fmt_args = common::Command::Fmt {
    check: false,
    staged: true,
    changed: false,
    lang: vec!["toml".to_string()],
    allow_missing: false,
    paths: vec![file_a.clone()],
  };
  assert_eq!(common::run_cli(root, &fmt_args), runner::ExitStatus::Clean);

  assert_eq!(
    fs::read_to_string(&file_a).unwrap(),
    "[package]\nname = \"a_mod\"\n"
  );
  assert_eq!(
    fs::read_to_string(&file_b).unwrap(),
    "[package]\n   name =   \"b_mod\"\n"
  );
}
