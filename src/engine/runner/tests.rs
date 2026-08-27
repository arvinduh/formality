use super::*;
use std::time::Duration;

#[test]
fn test_combine_fix_results_passed_and_skipped() {
  let lint_res = SurfaceResult {
    surface_name: "yaml",
    status: SurfaceStatus::Skipped {
      reason: "Tool does not support autofix".to_string(),
    },
    duration: Duration::from_millis(10),
  };
  let fmt_res = SurfaceResult {
    surface_name: "yaml",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(20),
  };

  let combined = combine_fix_results(lint_res, fmt_res);
  assert_eq!(combined.surface_name, "yaml");
  assert_eq!(combined.duration, Duration::from_millis(30));
  assert!(matches!(combined.status, SurfaceStatus::Passed));
}

#[test]
fn test_combine_fix_results_both_passed() {
  let lint_res = SurfaceResult {
    surface_name: "python",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(15),
  };
  let fmt_res = SurfaceResult {
    surface_name: "python",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(25),
  };

  let combined = combine_fix_results(lint_res, fmt_res);
  assert_eq!(combined.surface_name, "python");
  assert_eq!(combined.duration, Duration::from_millis(40));
  assert!(matches!(combined.status, SurfaceStatus::Passed));
}

#[test]
fn test_combine_fix_results_violations_precedence() {
  let lint_res = SurfaceResult {
    surface_name: "rust",
    status: SurfaceStatus::ViolationsFound {
      message: "warning: unused".to_string(),
      diff: None,
    },
    duration: Duration::from_millis(50),
  };
  let fmt_res = SurfaceResult {
    surface_name: "rust",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(30),
  };

  let combined = combine_fix_results(lint_res, fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ViolationsFound { message, .. } if message.contains("warning: unused")
  ));
}

#[test]
fn test_combine_fix_results_tool_missing_precedence() {
  let lint_res = SurfaceResult {
    surface_name: "python",
    status: SurfaceStatus::ToolMissing {
      binary: "ruff".to_string(),
      install_hint: "pip install ruff".to_string(),
    },
    duration: Duration::from_millis(5),
  };
  let fmt_res = SurfaceResult {
    surface_name: "python",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(5),
  };

  let combined = combine_fix_results(lint_res, fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ToolMissing { binary, .. } if binary == "ruff"
  ));
}

#[test]
fn test_combine_fix_results_execution_error_precedence() {
  let lint_res = SurfaceResult {
    surface_name: "cpp",
    status: SurfaceStatus::ExecutionError {
      message: "clang-tidy crashed".to_string(),
    },
    duration: Duration::from_millis(10),
  };
  let fmt_res = SurfaceResult {
    surface_name: "cpp",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(10),
  };

  let combined = combine_fix_results(lint_res, fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ExecutionError { message } if message.contains("clang-tidy crashed")
  ));
}

#[test]
fn test_runner_single_walk_polyglot_repo() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let fixture = manifest_dir.join("tests/fixtures/polyglot_repo");

  // Single candidate filesystem walk
  let candidates = crate::surfaces::walk_candidate_files(&fixture, &[]);
  assert!(
    candidates.len() >= 7,
    "Expected at least 7 files in polyglot_repo, found {}",
    candidates.len()
  );

  // Filter in-memory for each surface
  let rust_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::rust::RustSurface,
    &[],
    &[],
  );
  assert_eq!(rust_files.len(), 1);
  assert!(rust_files[0].ends_with("main.rs"));

  let py_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::python::PythonSurface,
    &[],
    &[],
  );
  assert_eq!(py_files.len(), 1);
  assert!(py_files[0].ends_with("script.py"));

  let md_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::markdown::MarkdownSurface,
    &[],
    &[],
  );
  assert_eq!(md_files.len(), 1);
  assert!(md_files[0].ends_with("README.md"));

  let yaml_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::yaml::YamlSurface,
    &[],
    &[],
  );
  assert_eq!(yaml_files.len(), 1);
  assert!(yaml_files[0].ends_with("config.yaml"));

  let json_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::json::JsonSurface,
    &[],
    &[],
  );
  assert_eq!(json_files.len(), 1);
  assert!(json_files[0].ends_with("data.json"));

  let typst_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::typst::TypstSurface,
    &[],
    &[],
  );
  assert_eq!(typst_files.len(), 1);
  assert!(typst_files[0].ends_with("doc.typ"));

  let toml_files = crate::surfaces::filter_files_for_surface(
    &candidates,
    &crate::surfaces::toml::TomlSurface,
    &[],
    &[],
  );
  assert_eq!(toml_files.len(), 1);
  assert!(toml_files[0].ends_with("Cargo.toml"));
}

#[test]
fn test_execution_context_candidate_files_filtering() {
  let candidates = Arc::new(vec![
    PathBuf::from("/ws/src/main.rs"),
    PathBuf::from("/ws/src/lib.rs"),
    PathBuf::from("/ws/src/ignored.rs"),
    PathBuf::from("/ws/script.py"),
  ]);

  let mut lang_config = crate::config::ResolvedLangConfig::new("rust");
  lang_config.exclude = vec![PathBuf::from("ignored.rs")];

  let ctx = ExecutionContext {
    root: Arc::new(PathBuf::from("/ws")),
    paths: Arc::new(Vec::new()),
    global_config: Arc::new(crate::config::ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: Some(candidates),
  };

  let matched = ctx.matched_files(&["rs"]);
  assert_eq!(matched.len(), 2);
  assert!(matched.contains(&PathBuf::from("/ws/src/main.rs")));
  assert!(matched.contains(&PathBuf::from("/ws/src/lib.rs")));
  assert!(!matched.contains(&PathBuf::from("/ws/src/ignored.rs")));
  assert!(!matched.contains(&PathBuf::from("/ws/script.py")));
}
