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

  let combined = combine_fix_results(lint_res, fmt_res, None);
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

  let combined = combine_fix_results(lint_res, fmt_res, None);
  assert_eq!(combined.surface_name, "python");
  assert_eq!(combined.duration, Duration::from_millis(40));
  assert!(matches!(combined.status, SurfaceStatus::Passed));
}

#[test]
fn test_combine_fix_results_recheck_clears_lint_violation() {
  // Issue #116: the lint pass reported a violation, but the post-format
  // re-check came back clean. The re-check supersedes the stale lint status,
  // so the surface reports Passed and its duration folds in all three passes.
  let lint_res = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::ViolationsFound {
      message: "MD013/line-length".to_string(),
      diff: None,
    },
    duration: Duration::from_millis(40),
  };
  let fmt_res = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(30),
  };
  let recheck = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(20),
  };

  let combined = combine_fix_results(lint_res, fmt_res, Some(recheck));
  assert!(matches!(combined.status, SurfaceStatus::Passed));
  assert_eq!(combined.duration, Duration::from_millis(90));
}

#[test]
fn test_combine_fix_results_recheck_preserves_surviving_violation() {
  // Issue #116 inverse: the violation survived the format pass, so the
  // re-check still reports it and the surface still fails.
  let lint_res = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::ViolationsFound {
      message: "MD025/single-title".to_string(),
      diff: None,
    },
    duration: Duration::from_millis(40),
  };
  let fmt_res = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::Passed,
    duration: Duration::from_millis(30),
  };
  let recheck = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::ViolationsFound {
      message: "MD025/single-title".to_string(),
      diff: None,
    },
    duration: Duration::from_millis(20),
  };

  let combined = combine_fix_results(lint_res, fmt_res, Some(recheck));
  assert!(matches!(
    combined.status,
    SurfaceStatus::ViolationsFound { message, .. }
      if message.contains("MD025")
  ));
  assert_eq!(combined.duration, Duration::from_millis(90));
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

  let combined = combine_fix_results(lint_res, fmt_res, None);
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

  let combined = combine_fix_results(lint_res, fmt_res, None);
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

  let combined = combine_fix_results(lint_res, fmt_res, None);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ExecutionError { message } if message.contains("clang-tidy crashed")
  ));
}

#[test]
fn test_normalize_diagnostics_keeps_error_signal_lines() {
  // Issue #146: normalization must de-noise (trailing whitespace, blank lines,
  // formatter banners) but never truncate or drop lines an ExecutionError
  // message needs -- the synthesized "Command failed" line and every stack
  // frame have to survive.
  let raw = "Checking formatting...\n\n  panic: runtime error   \n\ngoroutine 1 [running]:\nmain.main()\n\tmain.go:7 +0x1d\n\nCommand failed with exit code 2\n";
  let normalized = normalize_diagnostics(raw);
  assert_eq!(
    normalized,
    "  panic: runtime error\ngoroutine 1 [running]:\nmain.main()\n\tmain.go:7 +0x1d\nCommand failed with exit code 2"
  );
  assert!(normalized.contains("Command failed with exit code 2"));
  assert!(normalized.contains("main.go:7 +0x1d"));
  assert!(!normalized.contains("Checking formatting..."));
}

#[test]
fn test_execution_error_and_violations_render_detail_identically() {
  // Issue #146: identical raw tool output must produce byte-identical rendered
  // detail regardless of which status arm it lands in. This calls
  // `tool_output_detail` directly -- the exact function both the
  // `ViolationsFound` and `ExecutionError` arms in `Runner::run` call to
  // build the pushed diagnostic -- rather than re-deriving the arms' logic
  // here.
  //
  // This pins the helper's behavior, NOT the call-site wiring: un-wiring
  // either arm still passes. See the helper's doc comment.
  let raw = "Checking formatting...\n\nsrc/x.js: error   \n  2:1  Delete `;`\n\nAll checks passed!\nCommand failed with exit code 2\n";

  // ViolationsFound with no diff, and ExecutionError, both pass `diff: None`
  // through to `tool_output_detail`, so one call covers both arms' input.
  let exec_error_detail = tool_output_detail(raw, None);

  assert_eq!(
    exec_error_detail,
    "src/x.js: error\n  2:1  Delete `;`\nCommand failed with exit code 2"
  );
}

#[test]
fn test_tool_output_detail_prefers_diff_over_message() {
  // ViolationsFound's `Some(diff)` branch renders the diff verbatim,
  // bypassing normalize_diagnostics entirely -- assert that precedence here
  // rather than only through `normalize_diagnostics`'s own unit test.
  let detail = tool_output_detail(
    "Checking formatting...\nraw message noise",
    Some("- old\n+ new"),
  );
  assert_eq!(detail, "- old\n+ new");
}

#[test]
fn test_execution_error_arm_normalizes_via_runner_render() {
  // Issue #146: build a real ExecutionError SurfaceResult with noisy raw
  // tool output and check the detail `tool_output_detail` computes for it,
  // confirming an ExecutionError is normalized identically to a
  // ViolationsFound one.
  //
  // This calls the helper directly; it does NOT enter `Runner::run`, whose
  // rendering loop prints to stdout and is not observable from a test.
  let raw = "Checking formatting...\n\n  fatal: crashed   \n\nAll checks passed!\nCommand failed with exit code 2\n";
  let exec_result = SurfaceResult {
    surface_name: "go",
    status: SurfaceStatus::ExecutionError {
      message: raw.to_string(),
    },
    duration: Duration::from_millis(5),
  };

  let detail = match &exec_result.status {
    SurfaceStatus::ExecutionError { message } => {
      tool_output_detail(message, None)
    }
    _ => unreachable!(),
  };

  assert_eq!(detail, "  fatal: crashed\nCommand failed with exit code 2");
  assert!(!detail.contains("Checking formatting..."));
  assert!(!detail.contains("All checks passed!"));
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
