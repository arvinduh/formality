use fml::cli::{Cli, Commands};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_fix_command_rust_lifecycle() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();
  let src_dir = root.join("src");
  fs::create_dir_all(&src_dir).unwrap();

  fs::write(
    root.join("Cargo.toml"),
    "[package]\nname = \"fix_test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
  )
  .unwrap();

  // Create unformatted rust file
  let main_rs = src_dir.join("main.rs");
  fs::write(
    &main_rs,
    "fn main()   {\nprintln!(\"hello from fix\");\n}\n",
  )
  .unwrap();

  let fix_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["rust".to_string()],
      install: false,
      paths: vec![],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
  assert_eq!(exit_code, 0);

  let formatted = fs::read_to_string(&main_rs).unwrap();
  assert!(formatted.contains("fn main() {"));
  assert!(formatted.contains("println!(\"hello from fix\");"));

  // Subsequent check should be clean
  let check_args = Cli {
    config: None,
    root: Some(root),
    command: Commands::Fmt {
      check: true,
      staged: false,
      changed: false,
      lang: vec!["rust".to_string()],
      install: false,
      paths: vec![],
    },
  };
  assert_eq!(fml::run_with_args(check_args), 0);
}

#[test]
fn test_fix_command_targeted_paths() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();
  let sub_dir = root.join("nested");
  fs::create_dir_all(&sub_dir).unwrap();

  let target_file = sub_dir.join("target.toml");
  let untouched_file = root.join("untouched.toml");

  fs::write(&target_file, "[package]\n   name =   \"target\"\n").unwrap();
  let original_untouched = "[package]\n   name =   \"untouched\"\n";
  fs::write(&untouched_file, original_untouched).unwrap();

  let fix_args = Cli {
    config: None,
    root: Some(root),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["toml".to_string()],
      install: false,
      paths: vec![target_file.clone()],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
  assert_eq!(exit_code, 0);

  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target\""));

  let untouched_content = fs::read_to_string(&untouched_file).unwrap();
  assert_eq!(untouched_content, original_untouched);
}

#[test]
fn test_fix_command_unsupported_autofix_surfaces() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  // Create a TOML file (taplo formatter passes, lint fix skipped)
  let toml_file = root.join("sample.toml");
  fs::write(&toml_file, "[package]\n name = \"test\"\n").unwrap();

  let fix_args = Cli {
    config: None,
    root: Some(root),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["toml".to_string()],
      install: false,
      paths: vec![],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
  assert_eq!(exit_code, 0);

  let formatted = fs::read_to_string(&toml_file).unwrap();
  assert!(formatted.contains("name = \"test\""));
}

#[test]
fn test_fix_command_invalid_surface_and_mutual_exclusion() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  // 1. Invalid language surface filter returns error
  let invalid_lang_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["nonexistent_lang".to_string()],
      install: false,
      paths: vec![],
    },
  };
  assert_eq!(fml::run_with_args(invalid_lang_args), 2);

  // 2. Both staged and changed returns error
  let conflict_args = Cli {
    config: None,
    root: Some(root),
    command: Commands::Fix {
      staged: true,
      changed: true,
      lang: vec![],
      install: false,
      paths: vec![],
    },
  };
  assert_eq!(fml::run_with_args(conflict_args), 2);
}

#[test]
fn test_fix_command_polyglot_detection() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let fixture = manifest_dir.join("tests/fixtures/polyglot_repo");

  let fix_args = Cli {
    config: None,
    root: Some(fixture),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["toml".to_string()],
      install: false,
      paths: vec![],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
  assert_eq!(exit_code, 0);
}

#[test]
fn test_fix_command_staged_with_explicit_paths_filtering() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  let init_ok = std::process::Command::new("git")
    .arg("init")
    .current_dir(&root)
    .output()
    .map(|o| o.status.success())
    .unwrap_or(false);
  if !init_ok {
    return;
  }
  let _ = std::process::Command::new("git")
    .args(["config", "user.name", "test"])
    .current_dir(&root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["config", "user.email", "test@example.com"])
    .current_dir(&root)
    .output();

  let sub_dir = root.join("nested");
  fs::create_dir_all(&sub_dir).unwrap();

  let target_file = sub_dir.join("target.toml");
  let other_file = root.join("other.toml");
  let unformatted = "[package]\n   name =   \"test\"\n";

  fs::write(&target_file, unformatted).unwrap();
  fs::write(&other_file, unformatted).unwrap();

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(&root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["commit", "-m", "initial"])
    .current_dir(&root)
    .output();

  // Modify both files, stage both
  fs::write(&target_file, "[package]\n   name =   \"target_mod\"\n").unwrap();
  fs::write(&other_file, "[package]\n   name =   \"other_mod\"\n").unwrap();

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(&root)
    .output();

  // Run fix with staged: true AND explicit paths: [target_file]
  let fix_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: true,
      changed: false,
      lang: vec!["toml".to_string()],
      install: false,
      paths: vec![target_file.clone()],
    },
  };
  let exit_code = fml::run_with_args(fix_args);
  assert_eq!(exit_code, 0);

  // target_file should have been formatted
  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target_mod\""));

  // other_file was also staged, but because explicit paths were specified, it was not formatted!
  let unformatted_other = fs::read_to_string(&other_file).unwrap();
  assert_eq!(unformatted_other, "[package]\n   name =   \"other_mod\"\n");
}

#[test]
fn test_fix_command_changed_with_explicit_paths_filtering() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  let init_ok = std::process::Command::new("git")
    .arg("init")
    .current_dir(&root)
    .output()
    .map(|o| o.status.success())
    .unwrap_or(false);
  if !init_ok {
    return;
  }
  let _ = std::process::Command::new("git")
    .args(["config", "user.name", "test"])
    .current_dir(&root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["config", "user.email", "test@example.com"])
    .current_dir(&root)
    .output();

  let sub_dir = root.join("nested");
  fs::create_dir_all(&sub_dir).unwrap();

  let target_file = sub_dir.join("target.toml");
  let other_file = root.join("other.toml");

  fs::write(&target_file, "[package]\nname = \"target\"\n").unwrap();
  fs::write(&other_file, "[package]\nname = \"other\"\n").unwrap();

  let _ = std::process::Command::new("git")
    .args(["add", "."])
    .current_dir(&root)
    .output();
  let _ = std::process::Command::new("git")
    .args(["commit", "-m", "initial"])
    .current_dir(&root)
    .output();

  // Modify both files (unstaged)
  fs::write(&target_file, "[package]\n   name =   \"target_mod\"\n").unwrap();
  fs::write(&other_file, "[package]\n   name =   \"other_mod\"\n").unwrap();

  // Run fix with changed: true AND explicit paths: [target_file]
  let fix_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: false,
      changed: true,
      lang: vec!["toml".to_string()],
      install: false,
      paths: vec![target_file.clone()],
    },
  };
  let exit_code = fml::run_with_args(fix_args);
  assert_eq!(exit_code, 0);

  // target_file should have been formatted
  let formatted_target = fs::read_to_string(&target_file).unwrap();
  assert!(formatted_target.contains("name = \"target_mod\""));

  // other_file was also changed, but because explicit paths were specified, it was not formatted!
  let unformatted_other = fs::read_to_string(&other_file).unwrap();
  assert_eq!(unformatted_other, "[package]\n   name =   \"other_mod\"\n");
}

#[test]
fn test_fix_command_python_composite_lifecycle() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  let script_py = root.join("main.py");
  fs::write(
    &script_py,
    "import sys\nimport os\n\ndef foo(   x,  y  ):\n    return x+y\n",
  )
  .unwrap();

  let fix_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["python".to_string()],
      install: false,
      paths: vec![],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
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
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  let script_js = root.join("index.js");
  fs::write(&script_js, "function   add(  a, b )  {\nreturn a + b;\n}\n")
    .unwrap();

  let fix_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["javascript".to_string()],
      install: false,
      paths: vec![],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
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
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  let readme_md = root.join("README.md");
  fs::write(&readme_md, "# Title\n\nSome paragraph   with   spaces.\n")
    .unwrap();

  let fix_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fix {
      staged: false,
      changed: false,
      lang: vec!["markdown".to_string()],
      install: false,
      paths: vec![],
    },
  };

  let exit_code = fml::run_with_args(fix_args);
  if fml::surfaces::check_binary_exists("prettier") {
    assert_eq!(exit_code, 0);
    let formatted = fs::read_to_string(&readme_md).unwrap();
    assert!(formatted.contains("# Title"));
  } else {
    assert_ne!(exit_code, 0);
  }
}
