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
