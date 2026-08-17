use fml::cli::{Cli, Commands};
use fml::surfaces::{
  SurfaceRegistry, all_surfaces, detect_surfaces, get_surface_by_name,
  resolve_canonical_name,
};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_surface_registry_and_aliases() {
  let surfaces = all_surfaces();
  assert_eq!(surfaces.len(), 8);

  let registry = SurfaceRegistry::default();
  assert_eq!(registry.len(), 8);
  assert_eq!(
    registry.supported_languages(),
    vec![
      "rust", "python", "cpp", "markdown", "yaml", "json", "toml", "typst"
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
  ];

  for (query, canonical) in cases {
    let surface = get_surface_by_name(query);
    assert!(surface.is_some(), "Lookup failed for query '{}'", query);
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

  // Web fixture
  let web_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/web_repo"));
  let web_names: Vec<&str> = web_detected.iter().map(|s| s.name()).collect();
  assert!(web_names.contains(&"markdown"));
  assert!(web_names.contains(&"yaml"));
  assert!(web_names.contains(&"json"));

  // Typst fixture
  let typ_detected =
    detect_surfaces(&manifest_dir.join("tests/fixtures/typst_repo"));
  let typ_names: Vec<&str> = typ_detected.iter().map(|s| s.name()).collect();
  assert!(typ_names.contains(&"typst"));

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
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  // Create a dummy Python file to trigger detection
  fs::write(root.join("script.py"), "print('hi')").unwrap();

  // 1. Default init creates formality.toml
  let args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Init {
      force: false,
      hidden: false,
    },
  };

  let exit_code = fml::run_with_args(args);
  assert_eq!(exit_code, 0);

  let config_file = root.join("formality.toml");
  assert!(config_file.is_file());

  let content = fs::read_to_string(&config_file).unwrap();
  assert!(content.contains("[global]"));
  assert!(content.contains("languages = [\"python\"]"));

  // 2. Test --hidden creates .formality.toml with --force
  let args_hidden = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Init {
      force: true,
      hidden: true,
    },
  };
  let exit_code_hidden = fml::run_with_args(args_hidden);
  assert_eq!(exit_code_hidden, 0);
  assert!(root.join(".formality.toml").is_file());
}

#[test]
fn test_sync_config_workflow() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();

  // Set up dummy files for Rust, Python, C++, and Markdown
  fs::write(
    root.join("Cargo.toml"),
    "[package]\nname = \"dummy\"\nversion = \"0.1.0\"",
  )
  .unwrap();
  fs::write(root.join("pyproject.toml"), "[project]\nname = \"dummy\"")
    .unwrap();
  fs::write(root.join("CMakeLists.txt"), "project(dummy)").unwrap();
  fs::write(root.join("README.md"), "# Dummy").unwrap();
  fs::write(root.join("data.json"), "{}").unwrap();
  fs::write(root.join("config.toml"), "").unwrap();

  // 1. Initial sync --check should detect missing native files (drift)
  let check_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Sync {
      check: true,
      lang: vec![],
    },
  };
  let exit_drift = fml::run_with_args(check_args);
  assert_eq!(exit_drift, 1);

  // 2. Run sync (write mode)
  let write_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Sync {
      check: false,
      lang: vec![],
    },
  };
  let exit_write = fml::run_with_args(write_args);
  assert_eq!(exit_write, 0);

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
  let check_clean_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Sync {
      check: true,
      lang: vec![],
    },
  };
  let exit_clean = fml::run_with_args(check_clean_args);
  assert_eq!(exit_clean, 0);

  // 4. Manually drift a file and check that drift is reported
  fs::write(
    root.join(".rustfmt.toml"),
    "tab_spaces = 8\nmax_width = 120",
  )
  .unwrap();
  let check_after_tamper = Cli {
    config: None,
    root: Some(root),
    command: Commands::Sync {
      check: true,
      lang: vec!["rust".to_string()],
    },
  };
  let exit_tamper = fml::run_with_args(check_after_tamper);
  assert_eq!(exit_tamper, 1);
}

#[test]
fn test_list_surfaces_command() {
  let args = Cli {
    config: None,
    root: None,
    command: Commands::ListSurfaces,
  };
  let code = fml::run_with_args(args);
  assert_eq!(code, 0);
}

#[test]
fn test_doctor_command() {
  let args = Cli {
    config: None,
    root: None,
    command: Commands::Doctor {
      all: false,
      install: false,
    },
  };
  let _code = fml::run_with_args(args);
}

#[test]
fn test_schema_command() {
  let args = Cli {
    config: None,
    root: None,
    command: Commands::Schema { output: None },
  };
  let code = fml::run_with_args(args);
  assert_eq!(code, 0);

  let temp = tempfile::NamedTempFile::new().unwrap();
  let file_args = Cli {
    config: None,
    root: None,
    command: Commands::Schema {
      output: Some(temp.path().to_path_buf()),
    },
  };
  let file_code = fml::run_with_args(file_args);
  assert_eq!(file_code, 0);
  assert!(
    std::fs::read_to_string(temp.path())
      .unwrap()
      .contains("FormalityConfig")
  );
}

#[test]
fn test_fmt_and_lint_lifecycle() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();
  let src_dir = root.join("src");
  fs::create_dir_all(&src_dir).unwrap();

  fs::write(
    root.join("Cargo.toml"),
    "[package]\nname = \"lifecycle_test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
  )
  .unwrap();
  fs::write(
    src_dir.join("main.rs"),
    "fn main() {\nprintln!(\"hello\");\n}\n",
  )
  .unwrap();

  // 1. Format the codebase
  let fmt_write_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fmt {
      check: false,
      staged: false,
      changed: false,
      lang: vec!["rust".to_string()],
      install: false,
      paths: vec![],
    },
  };
  let write_code = fml::run_with_args(fmt_write_args);
  assert_eq!(write_code, 0);

  // 2. Check formatting (should be clean now)
  let fmt_check_args = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fmt {
      check: true,
      staged: false,
      changed: false,
      lang: vec!["rust".to_string()],
      install: false,
      paths: vec![],
    },
  };
  let check_code = fml::run_with_args(fmt_check_args);
  assert_eq!(check_code, 0);
}

#[test]
fn test_targeted_file_and_dir_formatting() {
  let temp = TempDir::new().unwrap();
  let root = temp.path().to_path_buf();
  let sub = root.join("nested");
  fs::create_dir_all(&sub).unwrap();

  let target_file = sub.join("target.rs");
  let other_file = root.join("untouched.rs");

  fs::write(&target_file, "fn target() {\nprintln!(\"target\");\n}\n").unwrap();
  fs::write(
    &other_file,
    "fn untouched() {\nprintln!(\"untouched\");\n}\n",
  )
  .unwrap();

  // Format only target_file
  let fmt_single = Cli {
    config: None,
    root: Some(root.clone()),
    command: Commands::Fmt {
      check: false,
      staged: false,
      changed: false,
      lang: vec!["rust".to_string()],
      install: false,
      paths: vec![target_file.clone()],
    },
  };
  let exit_code = fml::run_with_args(fmt_single);
  assert_eq!(exit_code, 0);

  // Format nested directory
  let fmt_dir = Cli {
    config: None,
    root: Some(root),
    command: Commands::Fmt {
      check: false,
      staged: false,
      changed: false,
      lang: vec!["rust".to_string()],
      install: false,
      paths: vec![sub],
    },
  };
  let exit_code_dir = fml::run_with_args(fmt_dir);
  assert_eq!(exit_code_dir, 0);
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
