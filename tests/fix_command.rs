mod common;

use common::{fix_cmd, fmt_cmd, init_git_repo, run_cli, temp_repo};
use fml::cli::Commands;
use std::fs;
use std::path::PathBuf;

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

  assert_eq!(run_cli(root, fix_cmd(&["rust"])), 0);

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

  assert_eq!(run_cli(root, fix_cmd(&["toml"])), 0);

  let formatted = fs::read_to_string(&toml_file).unwrap();
  assert!(formatted.contains("name = \"test\""));
}

#[test]
fn test_fix_command_invalid_surface_and_mutual_exclusion() {
  let temp = temp_repo(&[]);
  let root = temp.path();

  // 1. Invalid language surface filter returns error
  assert_eq!(run_cli(root, fix_cmd(&["nonexistent_lang"])), 2);

  // 2. Both staged and changed returns error
  let conflict_args = Commands::Fix {
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

  assert_eq!(run_cli(&fixture, fix_cmd(&["toml"])), 0);
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

  let exit_code = run_cli(root, fix_cmd(&["python"]));
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

  let exit_code = run_cli(root, fix_cmd(&["javascript"]));
  if fml::surfaces::check_binary_exists("biome") {
    assert_eq!(exit_code, 0);
    let formatted = fs::read_to_string(&script_js).unwrap();
    assert!(formatted.contains("function add(a, b) {"));
  } else {
    assert_ne!(exit_code, 0);
  }
}

#[test]
fn test_fix_command_markdown_composite_lifecycle() {
  let temp =
    temp_repo(&[("README.md", "# Title\n\nSome paragraph   with   spaces.\n")]);
  let root = temp.path();
  let readme_md = root.join("README.md");

  let exit_code = run_cli(root, fix_cmd(&["markdown"]));
  if fml::surfaces::check_binary_exists("prettier") {
    assert_eq!(exit_code, 0);
    let formatted = fs::read_to_string(&readme_md).unwrap();
    assert!(formatted.contains("# Title"));
  } else {
    assert_ne!(exit_code, 0);
  }
}
