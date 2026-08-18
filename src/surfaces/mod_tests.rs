use super::*;
use crate::surfaces::{
  cpp, go, java, javascript, json, kotlin, markdown, python, rust, toml, typst,
  yaml,
};

#[test]
fn test_surface_supports_lint_fix() {
  assert!(rust::RustSurface.supports_lint_fix());
  assert!(python::PythonSurface.supports_lint_fix());
  assert!(cpp::CppSurface.supports_lint_fix());
  assert!(!java::JavaSurface.supports_lint_fix());
  assert!(go::GoSurface.supports_lint_fix());
  assert!(!yaml::YamlSurface.supports_lint_fix());
  assert!(!toml::TomlSurface.supports_lint_fix());
  assert!(markdown::MarkdownSurface.supports_lint_fix());
  assert!(!json::JsonSurface.supports_lint_fix());
  assert!(!typst::TypstSurface.supports_lint_fix());
  assert!(javascript::JavaScriptSurface.supports_lint_fix());
  assert!(kotlin::KotlinSurface.supports_lint_fix());
}

#[test]
fn test_surface_result_predicates_cover_every_status_variant() {
  // SurfaceResult::is_success / is_violation / is_error are the tri-state
  // classification every downstream consumer (the runner's exit-code
  // logic, doctor summaries, table rendering) relies on. Each of the 7
  // SurfaceStatus variants had never been checked against these
  // predicates directly — only indirectly, via a handful of individual
  // surfaces' own integration tests.
  fn result_for(status: SurfaceStatus) -> SurfaceResult {
    SurfaceResult {
      surface_name: "test",
      status,
      duration: std::time::Duration::from_millis(0),
    }
  }

  let passed = result_for(SurfaceStatus::Passed);
  assert!(passed.is_success());
  assert!(!passed.is_violation());
  assert!(!passed.is_error());

  let skipped = result_for(SurfaceStatus::Skipped {
    reason: "n/a".to_string(),
  });
  assert!(skipped.is_success());
  assert!(!skipped.is_violation());
  assert!(!skipped.is_error());

  let synced = result_for(SurfaceStatus::ConfigSynced {
    file: "x".to_string(),
    created: true,
  });
  assert!(synced.is_success());
  assert!(!synced.is_violation());
  assert!(!synced.is_error());

  let violations = result_for(SurfaceStatus::ViolationsFound {
    message: "bad".to_string(),
    diff: None,
  });
  assert!(!violations.is_success());
  assert!(violations.is_violation());
  assert!(!violations.is_error());

  let drifted = result_for(SurfaceStatus::ConfigDrifted {
    file: "x".to_string(),
    diff: "d".to_string(),
  });
  assert!(!drifted.is_success());
  assert!(drifted.is_violation());
  assert!(!drifted.is_error());

  let manual = result_for(SurfaceStatus::ManualConfig {
    file: "x".to_string(),
    suggestion: "s".to_string(),
  });
  assert!(!manual.is_success());
  assert!(manual.is_violation());
  assert!(!manual.is_error());

  let missing = result_for(SurfaceStatus::ToolMissing {
    binary: "x".to_string(),
    install_hint: "h".to_string(),
  });
  assert!(!missing.is_success());
  assert!(!missing.is_violation());
  assert!(missing.is_error());

  let exec_err = result_for(SurfaceStatus::ExecutionError {
    message: "boom".to_string(),
  });
  assert!(!exec_err.is_success());
  assert!(!exec_err.is_violation());
  assert!(exec_err.is_error());
}

#[test]
fn test_box_dyn_language_surface_clone_preserves_identity() {
  // Clone for Box<dyn LanguageSurface> delegates to clone_box() on every
  // concrete surface; verify the round trip actually produces an
  // independent, equally-named clone rather than e.g. aliasing or
  // panicking, across a representative sample of surfaces.
  let originals: Vec<Box<dyn LanguageSurface>> = vec![
    Box::new(rust::RustSurface),
    Box::new(python::PythonSurface),
    Box::new(kotlin::KotlinSurface),
  ];

  for original in &originals {
    let cloned = original.clone();
    assert_eq!(cloned.name(), original.name());
    assert_eq!(cloned.file_extensions(), original.file_extensions());
  }
}

#[test]
fn test_unsupported_lint_fix_returns_skipped() {
  let dummy_ctx = ExecutionContext {
    root: PathBuf::from("."),
    paths: Arc::new(Vec::new()),
    global_config: Arc::new(ResolvedGlobalConfig::default()),
    lang_config: ResolvedLangConfig::new("dummy"),
    check_only: false,
  };

  let unsupported_surfaces: Vec<Box<dyn LanguageSurface>> = vec![
    Box::new(yaml::YamlSurface),
    Box::new(toml::TomlSurface),
    Box::new(json::JsonSurface),
    Box::new(typst::TypstSurface),
    Box::new(java::JavaSurface),
  ];

  for surface in unsupported_surfaces {
    let res = surface.lint(&dummy_ctx, true);
    match res.status {
      SurfaceStatus::Skipped { reason } => {
        assert_eq!(
          reason,
          "Tool does not support autofix; run fml fmt instead",
          "Mismatch for surface {}",
          surface.name()
        );
      }
      other => panic!(
        "Surface {} did not return Skipped on lint with fix=true: {:?}",
        surface.name(),
        other
      ),
    }
  }
}
