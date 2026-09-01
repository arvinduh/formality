mod common;

use common::{
  fmt_cmd, init_cmd, init_git_repo, run_cli, run_cli_no_root, sync_cmd,
  temp_repo,
};
use fml::cli::{Commands, MigrateCommands};
use fml::surfaces::{
  SurfaceRegistry, all_surfaces, detect_surfaces, get_surface_by_name,
  resolve_canonical_name,
};
use std::fs;
use std::path::PathBuf;

#[test]
fn test_surface_registry_and_aliases() {
  let surfaces = all_surfaces();
  assert_eq!(surfaces.len(), 12);

  let registry = SurfaceRegistry::default();
  assert_eq!(registry.len(), 12);
  assert_eq!(
    registry.supported_languages(),
    vec![
      "rust",
      "python",
      "cpp",
      "java",
      "go",
      "markdown",
      "yaml",
      "json",
      "toml",
      "typst",
      "javascript",
      "kotlin",
    ]
  );

  let cases = [
    ("rust", "rust"),
    ("rs", "rust"),
    ("RS", "rust"),
    ("python", "python"),
    ("py", "python"),
    ("Py", "python"),
    ("cpp", "cpp"),
    ("c", "cpp"),
    ("c++", "cpp"),
    ("C++", "cpp"),
    ("cxx", "cpp"),
    ("CXX", "cpp"),
    ("java", "java"),
    ("JAVA", "java"),
    ("jav", "java"),
    ("Java", "java"),
    ("go", "go"),
    ("GO", "go"),
    ("golang", "go"),
    ("markdown", "markdown"),
    ("md", "markdown"),
    ("MD", "markdown"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("YML", "yaml"),
    ("json", "json"),
    ("JSON", "json"),
    ("toml", "toml"),
    ("TOML", "toml"),
    ("typst", "typst"),
    ("typ", "typst"),
    ("TYP", "typst"),
    ("javascript", "javascript"),
    ("js", "javascript"),
    ("ts", "javascript"),
    ("typescript", "javascript"),
    ("kotlin", "kotlin"),
    ("kt", "kotlin"),
  ];

  for (query, canonical) in cases {
    let surface = get_surface_by_name(query);
    assert!(surface.is_some(), "Lookup failed for query '{query}'");
    assert_eq!(surface.unwrap().name(), canonical);
    assert_eq!(resolve_canonical_name(query), Some(canonical));

    let reg_surface = registry.get_surface_by_name(query);
    assert!(reg_surface.is_some());
    assert_eq!(reg_surface.unwrap().name(), canonical);
  }

  assert!(get_surface_by_name("nonexistent").is_none());
  assert!(resolve_canonical_name("nonexistent").is_none());
}

#[test]
fn test_surface_detection_in_fixtures() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

  // Rust fixture
  let rust_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/rust_repo"));
  let rust_names: Vec<&str> = rust_detected.iter().map(|s| s.name()).collect();
  assert!(rust_names.contains(&"rust"));

  // Python fixture
  let py_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/python_repo"));
  let py_names: Vec<&str> = py_detected.iter().map(|s| s.name()).collect();
  assert!(py_names.contains(&"python"));

  // C++ fixture
  let cpp_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/cpp_repo"));
  let cpp_names: Vec<&str> = cpp_detected.iter().map(|s| s.name()).collect();
  assert!(cpp_names.contains(&"cpp"));

  // Typst fixture
  let typ_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/typst_repo"));
  let typ_names: Vec<&str> = typ_detected.iter().map(|s| s.name()).collect();
  assert!(typ_names.contains(&"typst"));

  // Java fixture
  let java_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/java_repo"));
  let java_names: Vec<&str> = java_detected.iter().map(|s| s.name()).collect();
  assert!(java_names.contains(&"java"));

  // Go fixture
  let go_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/go_repo"));
  let go_names: Vec<&str> = go_detected.iter().map(|s| s.name()).collect();
  assert!(go_names.contains(&"go"));

  // Kotlin fixture
  let kotlin_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/kotlin_repo"));
  let kotlin_names: Vec<&str> =
    kotlin_detected.iter().map(|s| s.name()).collect();
  assert!(kotlin_names.contains(&"kotlin"));

  // JavaScript fixture
  let js_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/javascript_repo"));
  let js_names: Vec<&str> = js_detected.iter().map(|s| s.name()).collect();
  assert!(js_names.contains(&"javascript"));

  // TOML fixture
  let toml_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/toml_repo"));
  let toml_names: Vec<&str> = toml_detected.iter().map(|s| s.name()).collect();
  assert!(toml_names.contains(&"toml"));

  // Polyglot fixture
  let poly_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/polyglot_repo"));
  let poly_names: Vec<&str> = poly_detected.iter().map(|s| s.name()).collect();
  assert!(poly_names.contains(&"rust"));
  assert!(poly_names.contains(&"python"));
  assert!(poly_names.contains(&"markdown"));
  assert!(poly_names.contains(&"yaml"));
  assert!(poly_names.contains(&"json"));
  assert!(poly_names.contains(&"typst"));
}

#[test]
fn test_init_command() {
  let temp = temp_repo(&[("script.py", "print('hi')")]);
  let root = temp.path();

  // 1. Default init creates formality.toml
  assert_eq!(run_cli(root, init_cmd(false, false)), 0);

  let config_file = root.join("formality.toml");
  assert!(config_file.is_file());

  let content = fs::read_to_string(&config_file).unwrap();
  assert!(content.contains("[global]"));
  // auto-detect mode: no hardcoded languages list
  assert!(!content.contains("languages ="));
  assert!(content.contains("indent_size = 2"));

  // 2. Test --hidden creates .formality.toml with --force
  assert_eq!(run_cli(root, init_cmd(true, true)), 0);
  assert!(root.join(".formality.toml").is_file());
}

#[test]
fn test_sync_config_workflow() {
  let temp = temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"dummy\"\nversion = \"0.1.0\"",
    ),
    ("pyproject.toml", "[project]\nname = \"dummy\""),
    ("CMakeLists.txt", "project(dummy)"),
    ("README.md", "# Dummy"),
    ("data.json", "{}"),
    ("config.toml", ""),
  ]);
  let root = temp.path();

  // 1. Initial sync --check should detect missing native files (drift)
  assert_eq!(run_cli(root, sync_cmd(true, &[])), 1);

  // 2. Run sync (write mode)
  assert_eq!(run_cli(root, sync_cmd(false, &[])), 0);

  // Verify native files were created
  assert!(root.join(".rustfmt.toml").is_file());
  assert!(root.join("ruff.toml").is_file());
  assert!(root.join(".clang-format").is_file());
  assert!(root.join(".markdownlint.json").is_file());
  assert!(root.join(".prettierrc.json").is_file());
  assert!(root.join("taplo.toml").is_file());
  assert!(root.join(".editorconfig").is_file());

  // Check .editorconfig content
  let ec_content = fs::read_to_string(root.join(".editorconfig")).unwrap();
  assert!(ec_content.contains("root = true"));
  assert!(ec_content.contains("Auto-generated by formality. DO NOT EDIT."));
  assert!(ec_content.contains("[*]"));

  // Check .rustfmt.toml content
  let rustfmt_content = fs::read_to_string(root.join(".rustfmt.toml")).unwrap();
  assert!(rustfmt_content.contains("tab_spaces = 2"));
  assert!(rustfmt_content.contains("max_width = 80"));
  assert!(
    rustfmt_content.contains("Auto-generated by formality. DO NOT EDIT.")
  );
  assert!(rustfmt_content.contains("edition = \"2024\""));
  assert!(rustfmt_content.contains("reorder_imports = true"));

  // Check ruff.toml content
  let ruff_content = fs::read_to_string(root.join("ruff.toml")).unwrap();
  assert!(ruff_content.contains("line-length = 80"));
  assert!(ruff_content.contains("indent-width = 2"));
  assert!(ruff_content.contains("Auto-generated by formality. DO NOT EDIT."));

  // Check .clang-format content
  let clang_content = fs::read_to_string(root.join(".clang-format")).unwrap();
  assert!(clang_content.contains("IndentWidth: 2"));
  assert!(clang_content.contains("ColumnLimit: 80"));
  assert!(clang_content.contains("Auto-generated by formality. DO NOT EDIT."));

  // 3. Now sync --check should pass completely
  assert_eq!(run_cli(root, sync_cmd(true, &[])), 0);

  // 4. Manually drift a file and check that drift is reported
  fs::write(
    root.join(".rustfmt.toml"),
    "tab_spaces = 8\nmax_width = 120",
  )
  .unwrap();
  assert_eq!(run_cli(root, sync_cmd(true, &["rust"])), 1);
}

#[test]
fn test_list_surfaces_command() {
  assert_eq!(run_cli_no_root(Commands::ListSurfaces), 0);
}

#[test]
fn test_doctor_command() {
  let _code = run_cli_no_root(Commands::Doctor {
    all: false,
    install: false,
  });
}

#[test]
fn test_doctor_command_prints_sync_optional_notice() {
  use std::process::Command;

  let output = Command::new(env!("CARGO_BIN_EXE_fml"))
    .arg("doctor")
    .output()
    .expect("failed to run fml doctor");

  let stdout = String::from_utf8_lossy(&output.stdout);
  // `fml doctor` now wraps notice prose to the frame width, so the notice text
  // can carry hard line breaks — compare with all whitespace runs collapsed.
  let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
  let flat = normalize(&stdout);
  assert!(
    stdout.contains("fml sync:"),
    "expected doctor output to contain the 'fml sync:' notice heading, \
     got:\n{stdout}"
  );
  assert!(
    flat.contains(&normalize(fml::commands::doctor::SYNC_NOTICE_SUMMARY)),
    "expected doctor output to contain the sync-optional summary, \
     got:\n{stdout}"
  );
  assert!(
    flat.contains(&normalize(fml::commands::doctor::SYNC_NOTICE_DETAIL)),
    "expected doctor output to contain the sync-optional detail line, \
     got:\n{stdout}"
  );
}

#[test]
fn test_schema_command() {
  assert_eq!(run_cli_no_root(Commands::Schema { output: None }), 0);

  let temp = tempfile::NamedTempFile::new().unwrap();
  assert_eq!(
    run_cli_no_root(Commands::Schema {
      output: Some(temp.path().to_path_buf()),
    }),
    0
  );
  assert!(
    std::fs::read_to_string(temp.path())
      .unwrap()
      .contains("FormalityConfig")
  );
}

#[test]
fn test_migrate_schema_command() {
  let temp = temp_repo(&[]);
  let root = temp.path();

  // 1. No config present -> error.
  assert_eq!(
    run_cli(
      root,
      Commands::Migrate {
        command: MigrateCommands::Schema,
      }
    ),
    2
  );

  // 2. Stale #:schema line gets rewritten to the current version.
  fs::write(
    root.join("formality.toml"),
    "#:schema \
     https://github.com/arvinduh/formality/releases/download/s0.9/formality.schema.json\n[global]\nindent_size \
     = 2\n",
  )
  .unwrap();

  assert_eq!(
    run_cli(
      root,
      Commands::Migrate {
        command: MigrateCommands::Schema,
      }
    ),
    0
  );

  let content = fs::read_to_string(root.join("formality.toml")).unwrap();
  assert!(
    content
      .contains(&format!("s{}/formality.schema.json", fml::SCHEMA_VERSION))
  );
  assert!(content.contains("[global]\nindent_size = 2\n"));

  // 3. Already up to date -> no-op, file unchanged.
  assert_eq!(
    run_cli(
      root,
      Commands::Migrate {
        command: MigrateCommands::Schema,
      }
    ),
    0
  );
  let content_after = fs::read_to_string(root.join("formality.toml")).unwrap();
  assert_eq!(content, content_after);
}

#[test]
fn test_fmt_and_lint_lifecycle() {
  let temp = temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"lifecycle_test\"\nversion = \"0.1.0\"\nedition = \
       \"2024\"\n",
    ),
    ("src/main.rs", "fn main() {\nprintln!(\"hello\");\n}\n"),
  ]);
  let root = temp.path();

  // 1. Format the codebase
  assert_eq!(run_cli(root, fmt_cmd(false, &["rust"])), 0);

  // 2. Check formatting (should be clean now)
  assert_eq!(run_cli(root, fmt_cmd(true, &["rust"])), 0);
}

#[test]
fn test_targeted_file_and_dir_formatting() {
  let temp = temp_repo(&[
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
  let fmt_single = Commands::Fmt {
    check: false,
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    install: false,
    paths: vec![target_file],
  };
  assert_eq!(run_cli(root, fmt_single), 0);

  // Format nested directory
  let fmt_dir = Commands::Fmt {
    check: false,
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    install: false,
    paths: vec![sub],
  };
  assert_eq!(run_cli(root, fmt_dir), 0);
}

#[test]
fn test_ignore_languages_filtering() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let poly_root = manifest_dir.join("tests/fixtures/polyglot_repo");

  let config_str = r#"
    [global]
    ignore_languages = ["markdown", "yaml", "json"]
  "#;
  let config = fml::config::FormalityConfig::parse_str(
    config_str,
    std::path::Path::new("formality.toml"),
  )
  .unwrap();

  let detected = fml::surfaces::detect_surfaces_smart(&poly_root, &config);
  let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

  assert!(names.contains(&"rust"));
  assert!(names.contains(&"python"));
  assert!(!names.contains(&"markdown"));
  assert!(!names.contains(&"yaml"));
  assert!(!names.contains(&"json"));
}

#[test]
fn test_autodetect_all_workspace_surfaces_by_default() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let poly_root = manifest_dir.join("tests/fixtures/polyglot_repo");

  // Default config without explicit languages list — auto-detect mode
  let config = fml::config::FormalityConfig::with_defaults();
  assert_eq!(config.resolve_global().languages, None);

  let detected = fml::surfaces::detect_surfaces_smart(&poly_root, &config);
  let names: Vec<&str> = detected.iter().map(|s| s.name()).collect();

  assert!(names.contains(&"rust"));
  assert!(names.contains(&"python"));
  assert!(names.contains(&"markdown"));
  assert!(names.contains(&"toml"));
  assert!(names.contains(&"yaml"));
  assert!(names.contains(&"json"));
}

#[test]
fn test_fmt_python_import_sorting_lifecycle() {
  if which::which("ruff").is_err() {
    eprintln!(
      "Skipping test_fmt_python_import_sorting_lifecycle: ruff not installed \
       in PATH"
    );
    return;
  }

  let temp = temp_repo(&[
    ("pyproject.toml", "[project]\nname = \"test\"\n"),
    ("main.py", "import sys\nimport os\n\ndef greet():\n  pass\n"),
  ]);
  let root = temp.path();
  let py_file = root.join("main.py");

  // 1. fmt --check should fail because imports are not sorted
  assert_eq!(run_cli(root, fmt_cmd(true, &["python"])), 1);

  // 2. fmt (write mode) should sort imports
  assert_eq!(run_cli(root, fmt_cmd(false, &["python"])), 0);

  let formatted = fs::read_to_string(&py_file).unwrap();
  let os_pos = formatted.find("import os").expect("import os present");
  let sys_pos = formatted.find("import sys").expect("import sys present");
  assert!(
    os_pos < sys_pos,
    "import os must precede import sys after sorting"
  );

  // 3. fmt --check should now pass
  assert_eq!(run_cli(root, fmt_cmd(true, &["python"])), 0);
}

#[test]
fn test_fmt_rust_import_reordering_lifecycle() {
  let temp = temp_repo(&[
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
  assert_eq!(run_cli(root, fmt_cmd(true, &["rust"])), 1);

  // 2. fmt (write mode) should reorder imports
  assert_eq!(run_cli(root, fmt_cmd(false, &["rust"])), 0);

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
  assert_eq!(run_cli(root, fmt_cmd(true, &["rust"])), 0);
}

#[test]
fn test_fmt_fix_lint_doctor_install_flag_paths() {
  let temp = temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"install_flag_test\"\nversion = \"0.1.0\"\nedition \
       = \"2024\"\n",
    ),
    // 2-space indent: matches formality's own default `indent_size` (no
    // formality.toml present here, so the built-in default applies), not
    // rustfmt's own 4-space default. Before #151, plain `fml fmt --check`
    // silently checked against rustfmt's bare default instead of formality's
    // resolved config, so a 4-space fixture passed by coincidence; now that
    // the resolved config is actually applied inline, the fixture has to
    // already match it for `--check` to report clean.
    (
      "src/main.rs",
      "fn main() {\n  println!(\"Hello, world!\");\n}\n",
    ),
  ]);
  let root = temp.path();

  // Test doctor with install: true
  let doc_code = run_cli(
    root,
    Commands::Doctor {
      all: false,
      install: true,
    },
  );
  assert!(doc_code == 0 || doc_code == 2);

  // Test fmt with install: true
  let fmt_args = Commands::Fmt {
    check: true,
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    install: true,
    paths: vec![],
  };
  assert_eq!(run_cli(root, fmt_args), 0);

  // Test fix with install: true
  let fix_args = Commands::Fix {
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    install: true,
    paths: vec![],
  };
  assert_eq!(run_cli(root, fix_args), 0);

  // Test lint with install: true
  let lint_args = Commands::Lint {
    fix: false,
    staged: false,
    changed: false,
    lang: vec!["rust".to_string()],
    install: true,
    paths: vec![],
  };
  assert_eq!(run_cli(root, lint_args), 0);
}

#[test]
fn test_fmt_staged_and_changed_with_explicit_paths_filtering() {
  let temp = temp_repo(&[
    ("a.toml", "[package]\nname = \"a\"\n"),
    ("b.toml", "[package]\nname = \"b\"\n"),
  ]);
  let root = temp.path();

  if !init_git_repo(root) {
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
  let fmt_args = Commands::Fmt {
    check: false,
    staged: true,
    changed: false,
    lang: vec!["toml".to_string()],
    install: false,
    paths: vec![file_a.clone()],
  };
  assert_eq!(run_cli(root, fmt_args), 0);

  assert_eq!(
    fs::read_to_string(&file_a).unwrap(),
    "[package]\nname = \"a_mod\"\n"
  );
  assert_eq!(
    fs::read_to_string(&file_b).unwrap(),
    "[package]\n   name =   \"b_mod\"\n"
  );
}

#[test]
fn test_table_command_json_valid_and_invalid_syntax() {
  // 1. Valid table JSON payload
  let mut table = fml::ui::table::Table::new(vec![
    fml::ui::table::Column::new("Name"),
    fml::ui::table::Column::new("Status"),
  ]);
  table.add_row(fml::ui::table::Row::new(vec![
    fml::ui::table::Cell::text("Rust"),
    fml::ui::table::Cell::text("PASS"),
  ]));
  let valid_table_json = serde_json::to_string(&table).unwrap();

  assert_eq!(
    run_cli_no_root(Commands::Table {
      json: Some(valid_table_json),
    }),
    0
  );

  // 2. Empty JSON or missing structure
  assert_ne!(
    run_cli_no_root(Commands::Table {
      json: Some("[]".to_string()),
    }),
    0
  );

  // 3. Invalid JSON syntax
  assert_ne!(
    run_cli_no_root(Commands::Table {
      json: Some("{ not valid json }".to_string()),
    }),
    0
  );
}

#[test]
fn test_install_command_active_surfaces() {
  let temp = temp_repo(&[
    (
      "Cargo.toml",
      "[package]\nname = \"install_test\"\nversion = \"0.1.0\"\nedition = \
       \"2024\"\n",
    ),
    ("src/main.rs", "fn main() {}\n"),
  ]);
  let root = temp.path();

  // Install for active surfaces (rust is already installed or handled gracefully)
  assert_eq!(run_cli(root, Commands::Install { all: false }), 0);
}
