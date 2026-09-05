mod common;

use common::{fix_cmd, fmt_cmd, init_git_repo, lint_cmd, run_cli, temp_repo};
use fml::cli::Commands;
use std::fs;
use std::path::PathBuf;

/// `true` only when both tools the markdown surface drives are installed, so a
/// `fml fix` run actually exercises the lint pass *and* the format pass rather
/// than bailing out early with a `ToolMissing`.
fn markdown_toolchain_available() -> bool {
  fml::surfaces::check_binary_exists("prettier")
    && (fml::surfaces::check_binary_exists("markdownlint-cli2")
      || fml::surfaces::check_binary_exists("markdownlint"))
}

#[test]
fn test_fix_command_rust_lifecycle() {
  let temp = temp_repo(&[
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

  assert_eq!(run_cli(root, fix_cmd(false, &["rust"])), 0);

  let formatted = fs::read_to_string(&main_rs).unwrap();
  assert!(formatted.contains("fn main() {"));
  assert!(formatted.contains("println!(\"hello from fix\");"));

  // Subsequent check should be clean
  assert_eq!(run_cli(root, fmt_cmd(true, &["rust"])), 0);
}

#[test]
fn test_fix_command_targeted_paths() {
  let unformatted = "[package]\n   name =   \"target\"\n";
  let untouched = "[package]\n   name =   \"untouched\"\n";
  let temp = temp_repo(&[
    ("nested/target.toml", unformatted),
    ("untouched.toml", untouched),
  ]);
  let root = temp.path();
  let target_file = root.join("nested/target.toml");
  let untouched_file = root.join("untouched.toml");

  let fix_args = Commands::Fix {
    check: false,
    staged: false,
    changed: false,
    lang: vec!["toml".to_string()],
    install: false,
    paths: vec![target_file.clone()],
  };
  assert_eq!(run_cli(root, fix_args), 0);

  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target\""));

  let untouched_content = fs::read_to_string(&untouched_file).unwrap();
  assert_eq!(untouched_content, untouched);
}

#[test]
fn test_fix_command_unsupported_autofix_surfaces() {
  let temp = temp_repo(&[("sample.toml", "[package]\n name = \"test\"\n")]);
  let root = temp.path();
  let toml_file = root.join("sample.toml");

  assert_eq!(run_cli(root, fix_cmd(false, &["toml"])), 0);

  let formatted = fs::read_to_string(&toml_file).unwrap();
  assert!(formatted.contains("name = \"test\""));
}

#[test]
fn test_fix_command_invalid_surface_and_mutual_exclusion() {
  let temp = temp_repo(&[]);
  let root = temp.path();

  // 1. Invalid language surface filter returns error
  assert_eq!(run_cli(root, fix_cmd(false, &["nonexistent_lang"])), 2);

  // 2. Both staged and changed returns error
  let conflict_args = Commands::Fix {
    check: false,
    staged: true,
    changed: true,
    lang: vec![],
    install: false,
    paths: vec![],
  };
  assert_eq!(run_cli(root, conflict_args), 2);
}

#[test]
fn test_fix_command_polyglot_detection() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let fixture = manifest_dir.join("tests/fixtures/polyglot_repo");

  assert_eq!(run_cli(&fixture, fix_cmd(false, &["toml"])), 0);
}

#[test]
fn test_fix_command_staged_with_explicit_paths_filtering() {
  let temp = temp_repo(&[]);
  let root = temp.path();

  if !init_git_repo(root) {
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
  let fix_args = Commands::Fix {
    check: false,
    staged: true,
    changed: false,
    lang: vec!["toml".to_string()],
    install: false,
    paths: vec![target_file.clone()],
  };
  assert_eq!(run_cli(root, fix_args), 0);

  // target_file should have been formatted
  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target_mod\""));

  // other_file was also staged, but because explicit paths were specified, it was not formatted!
  let unformatted_other = fs::read_to_string(&other_file).unwrap();
  assert_eq!(unformatted_other, "[package]\n   name =   \"other_mod\"\n");
}

#[test]
fn test_fix_command_changed_with_explicit_paths_filtering() {
  let temp = temp_repo(&[]);
  let root = temp.path();

  if !init_git_repo(root) {
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
  let fix_args = Commands::Fix {
    check: false,
    staged: false,
    changed: true,
    lang: vec!["toml".to_string()],
    install: false,
    paths: vec![target_file.clone()],
  };
  assert_eq!(run_cli(root, fix_args), 0);

  // target_file should have been formatted
  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target_mod\""));

  // other_file was also changed, but because explicit paths were specified, it was not formatted!
  let unformatted_other = fs::read_to_string(&other_file).unwrap();
  assert_eq!(unformatted_other, "[package]\n   name =   \"other_mod\"\n");
}

#[test]
fn test_fix_command_python_composite_lifecycle() {
  let temp = temp_repo(&[(
    "main.py",
    "import sys\nimport os\n\ndef foo(   x,  y  ):\n    return x+y\n",
  )]);
  let root = temp.path();
  let script_py = root.join("main.py");

  let exit_code = run_cli(root, fix_cmd(false, &["python"]));
  if fml::surfaces::check_binary_exists("ruff") {
    assert_eq!(exit_code, 0);
    let formatted = fs::read_to_string(&script_py).unwrap();
    assert!(formatted.contains("def foo(x, y):"));
  } else {
    assert_ne!(exit_code, 0);
  }
}

#[test]
fn test_fix_command_javascript_composite_lifecycle() {
  let temp = temp_repo(&[(
    "index.js",
    "function   add(  a, b )  {\nreturn a + b;\n}\n",
  )]);
  let root = temp.path();
  let script_js = root.join("index.js");

  let exit_code = run_cli(root, fix_cmd(false, &["javascript"]));
  if fml::surfaces::check_binary_exists("biome") {
    assert_eq!(exit_code, 0);
    let formatted = fs::read_to_string(&script_js).unwrap();
    assert!(formatted.contains("function add(a, b) {"));
  } else {
    assert_ne!(exit_code, 0);
  }
}

/// Issue #116: the lint pass records an MD013 long-line violation that
/// markdownlint cannot auto-fix, then the format pass wraps the line with
/// prettier. `fml fix` must re-check the surface afterwards and report the
/// final, clean state: `[PASS]` and exit 0, with the file left correctly
/// wrapped.
#[test]
fn test_fix_command_reports_pass_when_format_pass_resolves_lint_violation() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: test_fix_command_reports_pass_when_format_pass_resolves_lint_violation \
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
  let temp = temp_repo(&[("doc.md", doc.as_str())]);
  let root = temp.path();
  let doc_md = root.join("doc.md");

  assert_eq!(run_cli(root, fix_cmd(false, &["markdown"])), 0);

  // prettier's prose-wrap pass (default `--prose-wrap=always`) rewrapped the
  // line, so the file on disk is now within the limit ...
  let formatted = fs::read_to_string(&doc_md).unwrap();
  assert!(formatted.contains("# Title"));
  assert!(
    formatted.lines().all(|l| l.chars().count() <= 80),
    "expected every line wrapped to <=80 cols, got:\n{formatted}"
  );

  // ... and a subsequent plain lint agrees the tree is clean.
  assert_eq!(run_cli(root, lint_cmd(false, &["markdown"])), 0);
}

/// Issue #116, inverse guard: a violation that *neither* pass can fix must
/// still fail. Two top-level headings trip MD025 — markdownlint has no
/// auto-fix for it and prettier does not merge headings — so `fml fix` must
/// still report `[FAIL]` and exit non-zero. The re-check must not turn into
/// "fix always succeeds".
#[test]
fn test_fix_command_still_fails_when_no_pass_resolves_violation() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: test_fix_command_still_fails_when_no_pass_resolves_violation \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let temp = temp_repo(&[("doc.md", "# First Heading\n\n# Second Heading\n")]);
  let root = temp.path();

  // Exit code 1 exactly (`ExitStatus::Violations`) — not merely non-zero: a
  // code of 2 (`ExitStatus::Error`) would mean the surface blew up rather
  // than reporting the surviving MD025 violation, which is a different and
  // wrong failure mode this guard must not accept.
  assert_eq!(run_cli(root, fix_cmd(false, &["markdown"])), 1);
}

#[test]
fn test_fix_command_markdown_composite_lifecycle() {
  let temp =
    temp_repo(&[("README.md", "# Title\n\nSome paragraph   with   spaces.\n")]);
  let root = temp.path();
  let readme_md = root.join("README.md");

  let exit_code = run_cli(root, fix_cmd(false, &["markdown"]));
  if fml::surfaces::check_binary_exists("prettier") {
    assert_eq!(exit_code, 0);
    let formatted = fs::read_to_string(&readme_md).unwrap();
    assert!(formatted.contains("# Title"));
  } else {
    assert_ne!(exit_code, 0);
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
fn test_fix_check_writes_nothing_and_flags_a_dirty_tree() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: test_fix_check_writes_nothing_and_flags_a_dirty_tree \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let long_line = "This is a long sentence of ordinary prose that will exceed \
                   the configured line length limit for sure and then some \
                   more words to be safe.";
  let doc = format!("# Title\n\n{long_line}\n");
  let temp = temp_repo(&[("doc.md", doc.as_str())]);
  let root = temp.path();
  let doc_md = root.join("doc.md");
  let before = fs::read_to_string(&doc_md).unwrap();

  // Reports the dirty tree ...
  assert_eq!(run_cli(root, fix_cmd(true, &["markdown"])), 1);

  // ... and wrote nothing while doing so. Byte-for-byte: a check run that
  // "only" normalized line endings would still be a write.
  assert_eq!(
    fs::read_to_string(&doc_md).unwrap(),
    before,
    "`fml fix --check` must not modify any file"
  );

  // The writing run does change it, and reports clean afterwards.
  assert_eq!(run_cli(root, fix_cmd(false, &["markdown"])), 0);
  assert_ne!(fs::read_to_string(&doc_md).unwrap(), before);
}

/// Issue #118: on an already-clean tree the check run and the write run
/// agree on exit status, and neither touches a byte.
///
/// This is the real invariant `fml fix --check` promises: it exits 0
/// exactly when `fml fix` would be a complete no-op.
#[test]
fn test_fix_check_and_fix_agree_on_an_already_clean_tree() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: test_fix_check_and_fix_agree_on_an_already_clean_tree \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let long_line = "This is a long sentence of ordinary prose that will exceed \
                   the configured line length limit for sure and then some \
                   more words to be safe.";
  let doc = format!("# Title\n\n{long_line}\n");
  let temp = temp_repo(&[("doc.md", doc.as_str())]);
  let root = temp.path();
  let doc_md = root.join("doc.md");

  // Bring the tree to the state `fml fix` leaves behind.
  assert_eq!(run_cli(root, fix_cmd(false, &["markdown"])), 0);
  let clean = fs::read_to_string(&doc_md).unwrap();

  let check_status = run_cli(root, fix_cmd(true, &["markdown"]));
  assert_eq!(
    fs::read_to_string(&doc_md).unwrap(),
    clean,
    "`fml fix --check` must not modify an already-clean file"
  );
  let write_status = run_cli(root, fix_cmd(false, &["markdown"]));

  assert_eq!(
    check_status, write_status,
    "check and write runs must agree on an already-clean tree"
  );
  assert_eq!(check_status, 0);
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
fn test_fix_check_and_fix_agree_when_no_pass_can_fix() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: test_fix_check_and_fix_agree_when_no_pass_can_fix \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let temp = temp_repo(&[("doc.md", "# First Heading\n\n# Second Heading\n")]);
  let root = temp.path();
  let doc_md = root.join("doc.md");
  let before = fs::read_to_string(&doc_md).unwrap();

  let check_status = run_cli(root, fix_cmd(true, &["markdown"]));
  let write_status = run_cli(root, fix_cmd(false, &["markdown"]));

  assert_eq!(
    check_status, write_status,
    "check and write runs must agree when nothing is fixable"
  );
  assert_eq!(check_status, 1);
  assert_eq!(
    fs::read_to_string(&doc_md).unwrap(),
    before,
    "neither run should have modified an unfixable file"
  );
}

/// Issue #118: `fml lint --fix` is the deprecated spelling of `fml fix` and
/// must keep working for one minor release — dispatching to the *fix* plan,
/// so the tree is left formatted rather than lint-fixed-but-unformatted.
#[test]
fn test_deprecated_lint_fix_runs_the_fix_plan() {
  if !markdown_toolchain_available() {
    eprintln!(
      "SKIP: test_deprecated_lint_fix_runs_the_fix_plan \
       — markdownlint/prettier not on PATH"
    );
    return;
  }

  let temp =
    temp_repo(&[("doc.md", "# Title\n\nSome paragraph   with   spaces.\n")]);
  let root = temp.path();
  let doc_md = root.join("doc.md");

  assert_eq!(run_cli(root, lint_cmd(true, &["markdown"])), 0);

  // The format pass ran: `fml lint --fix` used to leave this collapsed-run
  // of spaces alone, because it never reformatted.
  let after = fs::read_to_string(&doc_md).unwrap();
  assert!(
    !after.contains("   with   "),
    "`fml lint --fix` must dispatch to the fix plan, which reformats; got:\n{after}"
  );

  // And the tree it left behind is genuinely clean.
  assert_eq!(run_cli(root, fmt_cmd(true, &["markdown"])), 0);
}
