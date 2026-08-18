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
fn test_unsupported_lint_fix_returns_skipped() {
  let dummy_ctx = ExecutionContext {
    root: PathBuf::from("."),
    paths: Vec::new(),
    global_config: ResolvedGlobalConfig::default(),
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
