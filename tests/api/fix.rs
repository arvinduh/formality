//! End-to-end coverage for `fml fix`: per-surface lint-then-format
//! lifecycles, path targeting, `--staged`/`--changed` filtering, and the
//! post-fix re-check that reports what remains unfixed.

use fml::engine::runner;

use std::fs;
use std::path;

use crate::common;

/// `true` only when both tools the markdown surface drives are installed, so a
/// `fml fix` run actually exercises the lint pass *and* the format pass rather
/// than bailing out early with a `ToolMissing`.
fn markdown_toolchain_available() -> bool {
  fml::surfaces::tooling::check_binary_exists("prettier")
    && (fml::surfaces::tooling::check_binary_exists("markdownlint-cli2")
      || fml::surfaces::tooling::check_binary_exists("markdownlint"))
}

#[test]
fn rust_lifecycle() {
  let temp = common::temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"fix_test\"\nversion = \"0.1.0\"\nedition = \
       \"2024\"\n",
    ),
    (
      "src/main.rs",
      "fn main()   {\nprintln!(\"hello from fix\");\n}\n",
    ),
  ]);
  let root = temp.path();
  let main_rs = root.join("src/main.rs");

  assert_eq!(
    common::run_cli(root, &common::fix_cmd(false, &["rust"])),
    runner::ExitStatus::Clean
  );

  let formatted = fs::read_to_string(&main_rs).unwrap();
  assert!(formatted.contains("fn main() {"));
  assert!(formatted.contains("println!(\"hello from fix\");"));

  // Subsequent check should be clean
  assert_eq!(
    common::run_cli(root, &common::fmt_cmd(true, &["rust"])),
    runner::ExitStatus::Clean
  );
}

#[test]
fn targeted_paths() {
  let unformatted = "[package]\n   name =   \"target\"\n";
  let untouched = "[package]\n   name =   \"untouched\"\n";
  let temp = common::temp_repo(&[
    ("nested/target.toml", unformatted),
    ("untouched.toml", untouched),
  ]);
  let root = temp.path();
  let target_file = root.join("nested/target.toml");
  let untouched_file = root.join("untouched.toml");

  let fix_args = common::Command::Fix {
    check: false,
    staged: false,
    changed: false,
    lang: vec!["toml".to_string()],
    allow_missing: false,
    paths: vec![target_file.clone()],
  };
  assert_eq!(common::run_cli(root, &fix_args), runner::ExitStatus::Clean);

  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target\""));

  let untouched_content = fs::read_to_string(&untouched_file).unwrap();
  assert_eq!(untouched_content, untouched);
}

#[test]
fn unsupported_autofix_surfaces() {
  let temp =
    common::temp_repo(&[("sample.toml", "[package]\n name = \"test\"\n")]);
  let root = temp.path();
  let toml_file = root.join("sample.toml");

  let exit_code = common::run_cli(root, &common::fix_cmd(false, &["toml"]));
  if fml::surfaces::tooling::check_binary_exists("taplo") {
    assert_eq!(exit_code, runner::ExitStatus::Clean);
    let formatted = fs::read_to_string(&toml_file).unwrap();
    // The unformatted fixture already contains the substring
    // `name = "test"`, so `formatted.contains(..)` alone would pass even if
    // taplo never ran (vacuous). Assert the full reformatted content -- the
    // stray leading space before `name` is exactly what taplo strips --
    // so this actually verifies formatting happened.
    assert_eq!(formatted, "[package]\nname = \"test\"\n");
  } else {
    // ToolMissing is now loud: a project with TOML files and no taplo must
    // not exit clean (Fixes #252).
    assert_eq!(exit_code, runner::ExitStatus::Violations);
  }
}

#[test]
fn invalid_surface_and_mutual_exclusion() {
  let temp = common::temp_repo(&[]);
  let root = temp.path();

  // 1. Invalid language surface filter returns error
  assert_eq!(
    common::run_cli(root, &common::fix_cmd(false, &["nonexistent_lang"])),
    runner::ExitStatus::Error
  );

  // 2. Both staged and changed returns error
  let conflict_args = common::Command::Fix {
    check: false,
    staged: true,
    changed: true,
    lang: vec![],
    allow_missing: false,
    paths: vec![],
  };
  assert_eq!(
    common::run_cli(root, &conflict_args),
    runner::ExitStatus::Error
  );
}

#[test]
fn polyglot_detection() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let fixture = manifest_dir.join("tests/fixtures/polyglot_repo");
  let cargo_toml = fixture.join("Cargo.toml");
  let before = fs::read_to_string(&cargo_toml).unwrap();

  let exit_code = common::run_cli(&fixture, &common::fix_cmd(false, &["toml"]));
  if fml::surfaces::tooling::check_binary_exists("taplo") {
    assert_eq!(exit_code, runner::ExitStatus::Clean);
    // The fixture's Cargo.toml is already well-formatted, so taplo leaves
    // it byte-for-byte identical -- confirms the polyglot repo's toml
    // surface was actually detected and run, not skipped.
    let after = fs::read_to_string(&cargo_toml).unwrap();
    assert_eq!(after, before);
  } else {
    // ToolMissing is now loud: a project with TOML files and no taplo must
    // not exit clean (Fixes #252).
    assert_eq!(exit_code, runner::ExitStatus::Violations);
  }
}

#[test]
fn staged_with_explicit_paths_filtering() {
  let temp = common::temp_repo(&[]);
  let root = temp.path();

  if !common::init_git_repo(root) {
    return;
  }

  let sub_dir = root.join("nested");
  fs::create_dir_all(&sub_dir).unwrap();

  let target_file = sub_dir.join("target.toml");
  let other_file = root.join("other.toml");
  let unformatted = "[package]\n   name =   \"test\"\n";

  fs::write(&target_file, unformatted).unwrap();
  fs::write(&other_file, unformatted).unwrap();

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["commit", "-m", "initial"])
    .current_dir(root)
    .output();

  // Modify both files, stage both
  fs::write(&target_file, "[package]\n   name =   \"target_mod\"\n").unwrap();
  fs::write(&other_file, "[package]\n   name =   \"other_mod\"\n").unwrap();

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(root)
    .output();

  // Run fix with staged: true AND explicit paths: [target_file]
  let fix_args = common::Command::Fix {
    check: false,
    staged: true,
    changed: false,
    lang: vec!["toml".to_string()],
    allow_missing: false,
    paths: vec![target_file.clone()],
  };
  assert_eq!(common::run_cli(root, &fix_args), runner::ExitStatus::Clean);

  // target_file should have been formatted
  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target_mod\""));

  // other_file was also staged, but because explicit paths were specified, it was not formatted!
  let unformatted_other = fs::read_to_string(&other_file).unwrap();
  assert_eq!(unformatted_other, "[package]\n   name =   \"other_mod\"\n");
}

#[test]
fn changed_with_explicit_paths_filtering() {
  let temp = common::temp_repo(&[]);
  let root = temp.path();

  if !common::init_git_repo(root) {
    return;
  }

  let sub_dir = root.join("nested");
  fs::create_dir_all(&sub_dir).unwrap();

  let target_file = sub_dir.join("target.toml");
  let other_file = root.join("other.toml");

  fs::write(&target_file, "[package]\nname = \"target\"\n").unwrap();
  fs::write(&other_file, "[package]\nname = \"other\"\n").unwrap();

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["commit", "-m", "initial"])
    .current_dir(root)
    .output();

  // Modify both files (unstaged)
  fs::write(&target_file, "[package]\n   name =   \"target_mod\"\n").unwrap();
  fs::write(&other_file, "[package]\n   name =   \"other_mod\"\n").unwrap();

  // Run fix with changed: true AND explicit paths: [target_file]
  let fix_args = common::Command::Fix {
    check: false,
    staged: false,
    changed: true,
    lang: vec!["toml".to_string()],
    allow_missing: false,
    paths: vec![target_file.clone()],
  };
  assert_eq!(common::run_cli(root, &fix_args), runner::ExitStatus::Clean);

  // target_file should have been formatted
  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target_mod\""));

  // other_file was also changed, but because explicit paths were specified, it was not formatted!
  let unformatted_other = fs::read_to_string(&other_file).unwrap();
  assert_eq!(unformatted_other, "[package]\n   name =   \"other_mod\"\n");
}

#[test]
fn python_composite_lifecycle() {
  let temp = common::temp_repo(&[(
    "main.py",
    "import sys\nimport os\n\ndef foo(   x,  y  ):\n    return x+y\n",
  )]);
  let root = temp.path();
  let script_py = root.join("main.py");

  let exit_code = common::run_cli(root, &common::fix_cmd(false, &["python"]));
  if fml::surfaces::tooling::check_binary_exists("ruff") {
    assert_eq!(exit_code, runner::ExitStatus::Clean);
    let formatted = fs::read_to_string(&script_py).unwrap();
    assert!(formatted.contains("def foo(x, y):"));
  } else {
    // ToolMissing is now loud: a project with Python files and no ruff must
    // not exit clean (Fixes #252).
    assert_eq!(exit_code, runner::ExitStatus::Violations);
  }
}

#[test]
fn javascript_composite_lifecycle() {
  let temp = common::temp_repo(&[(
    "index.js",
    "function   add(  a, b )  {\nreturn a + b;\n}\n",
  )]);
  let root = temp.path();
  let script_js = root.join("index.js");

  let exit_code =
    common::run_cli(root, &common::fix_cmd(false, &["javascript"]));
  if fml::surfaces::tooling::check_binary_exists("biome") {
    assert_eq!(exit_code, runner::ExitStatus::Clean);
    let formatted = fs::read_to_string(&script_js).unwrap();
    assert!(formatted.contains("function add(a, b) {"));
  } else {
    // ToolMissing is now loud: a project with JS files and no biome must not
    // exit clean (Fixes #252).
    assert_eq!(exit_code, runner::ExitStatus::Violations);
  }
}

/// Issue #116: the lint pass records an MD013 long-line violation that
/// markdownlint cannot auto-fix, then the format pass wraps the line with
/// prettier. `fml fix` must re-check the surface afterwards and report the
/// final, clean state: `[PASS]` and exit 0, with the file left correctly
/// wrapped.
#[test]
fn reports_pass_when_format_pass_resolves_lint_violation() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: reports_pass_when_format_pass_resolves_lint_violation \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  // One over-long prose line (~135 cols) and nothing else wrong. markdownlint
  // flags MD013 (default line_length 80); MD013 is not in markdownlint's
  // auto-fixable set, so the lint pass genuinely records a violation.
  let long_line = "This is a long sentence of ordinary prose that will exceed \
                   the configured line length limit for sure and then some \
                   more words to be safe.";
  let doc = format!("# Title\n\n{long_line}\n");
  let temp = common::temp_repo(&[("doc.md", doc.as_str())]);
  let root = temp.path();
  let doc_md = root.join("doc.md");

  assert_eq!(
    common::run_cli(root, &common::fix_cmd(false, &["markdown"])),
    runner::ExitStatus::Clean
  );

  // prettier's prose-wrap pass (default `--prose-wrap=always`) rewrapped the
  // line, so the file on disk is now within the limit ...
  let formatted = fs::read_to_string(&doc_md).unwrap();
  assert!(formatted.contains("# Title"));
  assert!(
    formatted.lines().all(|l| l.chars().count() <= 80),
    "expected every line wrapped to <=80 cols, got:\n{formatted}"
  );

  // ... and a subsequent plain lint agrees the tree is clean.
  assert_eq!(
    common::run_cli(root, &common::lint_cmd(&["markdown"])),
    runner::ExitStatus::Clean
  );
}

/// Issue #116, inverse guard: a violation that *neither* pass can fix must
/// still fail. Two top-level headings trip MD025 — markdownlint has no
/// auto-fix for it and prettier does not merge headings — so `fml fix` must
/// still report `[FAIL]` and exit non-zero. The re-check must not turn into
/// "fix always succeeds".
#[test]
fn still_fails_when_no_pass_resolves_violation() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: still_fails_when_no_pass_resolves_violation \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let temp =
    common::temp_repo(&[("doc.md", "# First Heading\n\n# Second Heading\n")]);
  let root = temp.path();

  // Exit code 1 exactly (`ExitStatus::Violations`) — not merely non-zero: a
  // code of 2 (`ExitStatus::Error`) would mean the surface blew up rather
  // than reporting the surviving MD025 violation, which is a different and
  // wrong failure mode this guard must not accept.
  assert_eq!(
    common::run_cli(root, &common::fix_cmd(false, &["markdown"])),
    runner::ExitStatus::Violations
  );
}

#[test]
fn markdown_composite_lifecycle() {
  let temp = common::temp_repo(&[(
    "README.md",
    "# Title\n\nSome paragraph   with   spaces.\n",
  )]);
  let root = temp.path();
  let readme_md = root.join("README.md");

  let exit_code = common::run_cli(root, &common::fix_cmd(false, &["markdown"]));
  if markdown_toolchain_available() {
    assert_eq!(exit_code, runner::ExitStatus::Clean);
    let formatted = fs::read_to_string(&readme_md).unwrap();
    assert!(formatted.contains("# Title"));
  } else {
    // ToolMissing is now loud: a project with markdown files and a missing
    // prettier/markdownlint must not exit clean (Fixes #252).
    assert_eq!(exit_code, runner::ExitStatus::Violations);
  }
}

/// Issue #118: `fml fix --check` must write nothing. The lint pass runs
/// without `--fix` and the format pass runs check-only, so a dirty tree is
/// reported and left exactly as it was found.
///
/// This also pins the deliberate asymmetry between the two runs on a dirty
/// tree: `fix --check` answers "would this change?" (yes -> 1) while `fix`
/// answers "is anything still broken after fixing?" (no -> 0). They are
/// *supposed* to differ here; the invariant that actually holds is pinned
/// by the two tests below.
#[test]
fn check_writes_nothing_and_flags_a_dirty_tree() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: check_writes_nothing_and_flags_a_dirty_tree \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let long_line = "This is a long sentence of ordinary prose that will exceed \
                   the configured line length limit for sure and then some \
                   more words to be safe.";
  let doc = format!("# Title\n\n{long_line}\n");
  let temp = common::temp_repo(&[("doc.md", doc.as_str())]);
  let root = temp.path();
  let doc_md = root.join("doc.md");
  let before = fs::read_to_string(&doc_md).unwrap();

  // Reports the dirty tree ...
  assert_eq!(
    common::run_cli(root, &common::fix_cmd(true, &["markdown"])),
    runner::ExitStatus::Violations
  );

  // ... and wrote nothing while doing so. Byte-for-byte: a check run that
  // "only" normalized line endings would still be a write.
  assert_eq!(
    fs::read_to_string(&doc_md).unwrap(),
    before,
    "`fml fix --check` must not modify any file"
  );

  // The writing run does change it, and reports clean afterwards.
  assert_eq!(
    common::run_cli(root, &common::fix_cmd(false, &["markdown"])),
    runner::ExitStatus::Clean
  );
  assert_ne!(fs::read_to_string(&doc_md).unwrap(), before);
}

/// Issue #118: on an already-clean tree the check run and the write run
/// agree on exit status, and neither touches a byte.
///
/// This is the real invariant `fml fix --check` promises: it exits 0
/// exactly when `fml fix` would be a complete no-op.
#[test]
fn check_and_fix_agree_on_an_already_clean_tree() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: check_and_fix_agree_on_an_already_clean_tree \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let long_line = "This is a long sentence of ordinary prose that will exceed \
                   the configured line length limit for sure and then some \
                   more words to be safe.";
  let doc = format!("# Title\n\n{long_line}\n");
  let temp = common::temp_repo(&[("doc.md", doc.as_str())]);
  let root = temp.path();
  let doc_md = root.join("doc.md");

  // Bring the tree to the state `fml fix` leaves behind.
  assert_eq!(
    common::run_cli(root, &common::fix_cmd(false, &["markdown"])),
    runner::ExitStatus::Clean
  );
  let clean = fs::read_to_string(&doc_md).unwrap();

  let check_status =
    common::run_cli(root, &common::fix_cmd(true, &["markdown"]));
  assert_eq!(
    fs::read_to_string(&doc_md).unwrap(),
    clean,
    "`fml fix --check` must not modify an already-clean file"
  );
  let write_status =
    common::run_cli(root, &common::fix_cmd(false, &["markdown"]));

  assert_eq!(
    check_status, write_status,
    "check and write runs must agree on an already-clean tree"
  );
  assert_eq!(check_status, runner::ExitStatus::Clean);
  assert_eq!(fs::read_to_string(&doc_md).unwrap(), clean);
}

/// Issue #118: when no pass can fix the violation, the check run and the
/// write run agree on a *dirty* tree too — both exit 1.
///
/// Two top-level headings trip MD025, which markdownlint cannot auto-fix
/// and prettier does not merge. This is precisely the case that makes
/// "the check verdict matches whether the write run modified files" a false
/// invariant: the write run modifies nothing here and still exits 1.
#[test]
fn check_and_fix_agree_when_no_pass_can_fix() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: check_and_fix_agree_when_no_pass_can_fix \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let temp =
    common::temp_repo(&[("doc.md", "# First Heading\n\n# Second Heading\n")]);
  let root = temp.path();
  let doc_md = root.join("doc.md");
  let before = fs::read_to_string(&doc_md).unwrap();

  let check_status =
    common::run_cli(root, &common::fix_cmd(true, &["markdown"]));
  let write_status =
    common::run_cli(root, &common::fix_cmd(false, &["markdown"]));

  assert_eq!(
    check_status, write_status,
    "check and write runs must agree when nothing is fixable"
  );
  assert_eq!(check_status, runner::ExitStatus::Violations);
  assert_eq!(
    fs::read_to_string(&doc_md).unwrap(),
    before,
    "neither run should have modified an unfixable file"
  );
}

/// Issue #394: with `indent_size = 4`, prettier's `tabWidth` is 4, so prettier
/// indents a nested bullet list by 4 spaces. markdownlint's MD007 `indent`
/// must match so `fml fix` followed by `fml lint` passes without oscillation.
#[test]
fn markdown_nested_list_indent_sync() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: markdown_nested_list_indent_sync        — markdownlint/prettier not on PATH"
    );
    return;
  }

  for indent_size in [2, 4] {
    let config = format!(
      "[global]
indent_size = {indent_size}
"
    );
    let doc = "# Title

* item
    * nested
";
    let temp = common::temp_repo(&[
      ("formality.toml", config.as_str()),
      ("doc.md", doc),
    ]);
    let root = temp.path();

    assert_eq!(
      common::run_cli(root, &common::fix_cmd(false, &["markdown"])),
      runner::ExitStatus::Clean,
      "fml fix failed for indent_size = {indent_size}"
    );
    assert_eq!(
      common::run_cli(root, &common::lint_cmd(&["markdown"])),
      runner::ExitStatus::Clean,
      "subsequent fml lint failed for indent_size = {indent_size}"
    );
  }
}
