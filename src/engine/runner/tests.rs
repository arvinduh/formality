use super::*;
use std::time;

#[test]
fn test_combine_pass_results_passed_and_skipped() {
  let lint_res = surfaces::SurfaceResult {
    surface_name: "yaml",
    status: surfaces::SurfaceStatus::Skipped {
      reason: "Tool does not support autofix".to_string(),
    },
    duration: time::Duration::from_millis(10),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "yaml",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(20),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert_eq!(combined.surface_name, "yaml");
  assert_eq!(combined.duration, time::Duration::from_millis(30));
  assert!(matches!(combined.status, surfaces::SurfaceStatus::Passed));
}

#[test]
fn test_combine_pass_results_both_passed() {
  let lint_res = surfaces::SurfaceResult {
    surface_name: "python",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(15),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "python",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(25),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert_eq!(combined.surface_name, "python");
  assert_eq!(combined.duration, time::Duration::from_millis(40));
  assert!(matches!(combined.status, surfaces::SurfaceStatus::Passed));
}

#[test]
fn test_combine_pass_results_recheck_clears_lint_violation() {
  // Issue #116: the lint pass reported a violation, but the post-format
  // re-check came back clean. The re-check supersedes the stale lint status,
  // so the surface reports Passed and its duration folds in all three passes.
  let lint_res = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: "MD013/line-length".to_string(),
      diff: None,
    },
    duration: time::Duration::from_millis(40),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(30),
  };
  let recheck = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(20),
  };

  let combined =
    combine_pass_results(apply_recheck(lint_res, Some(recheck)), fmt_res);
  assert!(matches!(combined.status, surfaces::SurfaceStatus::Passed));
  assert_eq!(combined.duration, time::Duration::from_millis(90));
}

#[test]
fn test_combine_pass_results_recheck_preserves_surviving_violation() {
  // Issue #116 inverse: the violation survived the format pass, so the
  // re-check still reports it and the surface still fails.
  let lint_res = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: "MD025/single-title".to_string(),
      diff: None,
    },
    duration: time::Duration::from_millis(40),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(30),
  };
  let recheck = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: "MD025/single-title".to_string(),
      diff: None,
    },
    duration: time::Duration::from_millis(20),
  };

  let combined =
    combine_pass_results(apply_recheck(lint_res, Some(recheck)), fmt_res);
  assert!(matches!(
    combined.status,
    surfaces::SurfaceStatus::ViolationsFound { message, .. }
      if message.contains("MD025")
  ));
  assert_eq!(combined.duration, time::Duration::from_millis(90));
}

#[test]
fn test_combine_pass_results_violations_precedence() {
  let lint_res = surfaces::SurfaceResult {
    surface_name: "rust",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: "warning: unused".to_string(),
      diff: None,
    },
    duration: time::Duration::from_millis(50),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "rust",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(30),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    surfaces::SurfaceStatus::ViolationsFound { message, .. } if message.contains("warning: unused")
  ));
}

#[test]
fn test_combine_pass_results_tool_missing_precedence() {
  let lint_res = surfaces::SurfaceResult {
    surface_name: "python",
    status: surfaces::SurfaceStatus::ToolMissing {
      binary: "ruff".to_string(),
      install_hint: "pip install ruff".to_string(),
    },
    duration: time::Duration::from_millis(5),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "python",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(5),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    surfaces::SurfaceStatus::ToolMissing { binary, .. } if binary == "ruff"
  ));
}

#[test]
fn test_combine_pass_results_execution_error_precedence() {
  let lint_res = surfaces::SurfaceResult {
    surface_name: "cpp",
    status: surfaces::SurfaceStatus::ExecutionError {
      message: "clang-tidy crashed".to_string(),
    },
    duration: time::Duration::from_millis(10),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "cpp",
    status: surfaces::SurfaceStatus::Passed,
    duration: time::Duration::from_millis(10),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    surfaces::SurfaceStatus::ExecutionError { message } if message.contains("clang-tidy crashed")
  ));
}

/// One status per `surfaces::SurfaceStatus` variant, every payload tagged with `tag`,
/// ordered from lowest to highest `combine_pass_results` precedence.
fn every_status_by_precedence(tag: &str) -> Vec<surfaces::SurfaceStatus> {
  vec![
    surfaces::SurfaceStatus::Skipped {
      reason: tag.to_string(),
    },
    surfaces::SurfaceStatus::Passed,
    surfaces::SurfaceStatus::ConfigSynced {
      files: vec![surfaces::SyncedConfigFile::new(tag, true)],
    },
    surfaces::SurfaceStatus::ToolMissing {
      binary: tag.to_string(),
      install_hint: tag.to_string(),
    },
    surfaces::SurfaceStatus::ManualConfig {
      file: tag.to_string(),
      suggestion: tag.to_string(),
    },
    surfaces::SurfaceStatus::ConfigDrifted {
      file: tag.to_string(),
      diff: tag.to_string(),
    },
    surfaces::SurfaceStatus::ViolationsFound {
      message: tag.to_string(),
      diff: Some(tag.to_string()),
    },
    surfaces::SurfaceStatus::ExecutionError {
      message: tag.to_string(),
    },
  ]
}

/// The position `status` holds in [`every_status_by_precedence`].
///
/// Exhaustive with no wildcard, so a new `surfaces::SurfaceStatus` does not compile
/// until it is given a position here; placing it shifts every later arm,
/// which fails `test_every_status_by_precedence_lists_each_variant_in_order`
/// until the fixture lists it at that position too. A variant placed last
/// shifts nothing, and stable Rust cannot count an enum's variants, so that
/// one case still needs its fixture entry added by hand.
fn variant_index(status: &surfaces::SurfaceStatus) -> usize {
  match status {
    surfaces::SurfaceStatus::Skipped { .. } => 0,
    surfaces::SurfaceStatus::Passed => 1,
    surfaces::SurfaceStatus::ConfigSynced { .. } => 2,
    surfaces::SurfaceStatus::ToolMissing { .. } => 3,
    surfaces::SurfaceStatus::ManualConfig { .. } => 4,
    surfaces::SurfaceStatus::ConfigDrifted { .. } => 5,
    surfaces::SurfaceStatus::ViolationsFound { .. } => 6,
    surfaces::SurfaceStatus::ExecutionError { .. } => 7,
  }
}

#[test]
fn test_every_status_by_precedence_lists_each_variant_in_order() {
  let indices: Vec<usize> = every_status_by_precedence("x")
    .iter()
    .map(variant_index)
    .collect();
  assert_eq!(indices, (0..indices.len()).collect::<Vec<_>>());
}

fn combine_statuses(
  first: surfaces::SurfaceStatus,
  second: surfaces::SurfaceStatus,
) -> surfaces::SurfaceStatus {
  let result = |status| surfaces::SurfaceResult {
    surface_name: "test",
    status,
    duration: time::Duration::ZERO,
  };
  combine_pass_results(result(first), result(second)).status
}

#[test]
fn test_combine_pass_results_higher_precedence_wins_in_either_order() {
  let ranked = every_status_by_precedence("x");
  for (i, lower) in ranked.iter().enumerate() {
    for higher in &ranked[i + 1..] {
      for (first, second) in [(lower, higher), (higher, lower)] {
        let combined = combine_statuses(first.clone(), second.clone());
        assert_eq!(
          format!("{combined:?}"),
          format!("{higher:?}"),
          "{first:?} + {second:?}"
        );
      }
    }
  }
}

#[test]
fn test_combine_pass_results_same_variant_merges_or_keeps_first() {
  let firsts = every_status_by_precedence("a");
  let seconds = every_status_by_precedence("b");
  for (first, second) in firsts.into_iter().zip(seconds) {
    let expected = match &first {
      surfaces::SurfaceStatus::Skipped { .. } => {
        surfaces::SurfaceStatus::Skipped {
          reason: "a; b".to_string(),
        }
      }
      surfaces::SurfaceStatus::ViolationsFound { .. } => {
        surfaces::SurfaceStatus::ViolationsFound {
          message: "a\nb".to_string(),
          diff: Some("a\nb".to_string()),
        }
      }
      surfaces::SurfaceStatus::ExecutionError { .. } => {
        surfaces::SurfaceStatus::ExecutionError {
          message: "a\nb".to_string(),
        }
      }
      other => other.clone(),
    };
    let combined = combine_statuses(first, second);
    assert_eq!(format!("{combined:?}"), format!("{expected:?}"));
  }
}

#[test]
fn test_exit_floor_agrees_with_is_success_and_rises_with_precedence() {
  let mut previous_floor = 0;
  for status in every_status_by_precedence("x") {
    let floor = exit_floor(&status.severity(), false);
    let result = surfaces::SurfaceResult {
      surface_name: "test",
      status,
      duration: time::Duration::ZERO,
    };
    assert_eq!(result.is_success(), floor == 0, "{:?}", result.status);
    assert!(floor >= previous_floor, "{:?}", result.status);
    previous_floor = floor;
  }
}

#[test]
fn test_normalize_diagnostics_keeps_error_signal_lines() {
  // Issue #146, #179: normalization must de-noise (trailing whitespace,
  // consecutive blank lines, formatter banners) while preserving structural
  // single blank lines and error signals needed by ExecutionError messages.
  let raw = "Checking formatting...\n\n  panic: runtime error   \n\ngoroutine 1 [running]:\nmain.main()\n\tmain.go:7 +0x1d\n\nCommand failed with exit code 2\n";
  let normalized = normalize_diagnostics(raw);
  assert_eq!(
    normalized,
    "  panic: runtime error\n\ngoroutine 1 [running]:\nmain.main()\n\tmain.go:7 +0x1d\n\nCommand failed with exit code 2"
  );
  assert!(normalized.contains("Command failed with exit code 2"));
  assert!(normalized.contains("main.go:7 +0x1d"));
  assert!(!normalized.contains("Checking formatting..."));
}

#[test]
fn test_normalize_diagnostics_preserves_multierror_execution_error_grouping() {
  // Issue #179: multi-error diagnostics (e.g. rustc error blocks or Go panics)
  // use blank lines as structural grouping. normalize_diagnostics must preserve
  // single blank lines between error blocks while collapsing consecutive blank
  // lines and trimming leading/trailing blank noise.
  let rustc_output = "\
\n\nChecking formatting...\n\n\
error[E0425]: cannot find value `x` in this scope\n \
 --> src/main.rs:2:5\n  \
  |\n\
2 |     x + 1;\n  \
  |     ^ not found in this scope\n\n\n\
error[E0425]: cannot find value `y` in this scope\n \
 --> src/main.rs:3:5\n  \
  |\n\
3 |     y + 2;\n  \
  |     ^ not found in this scope\n\n\
error: aborting due to 2 previous errors\n\n\
Command failed with exit code 101\n\n";

  let normalized = normalize_diagnostics(rustc_output);
  let expected = "\
error[E0425]: cannot find value `x` in this scope\n \
 --> src/main.rs:2:5\n  \
  |\n\
2 |     x + 1;\n  \
  |     ^ not found in this scope\n\n\
error[E0425]: cannot find value `y` in this scope\n \
 --> src/main.rs:3:5\n  \
  |\n\
3 |     y + 2;\n  \
  |     ^ not found in this scope\n\n\
error: aborting due to 2 previous errors\n\n\
Command failed with exit code 101";

  assert_eq!(normalized, expected);
}

#[test]
fn test_normalize_diagnostics_blank_line_collapsing_and_trimming() {
  // Issue #179: edge cases in blank-line collapsing:
  // - runs of 2+ blank lines collapse to 1
  // - whitespace-only lines are treated as blank
  // - leading and trailing blank lines are stripped
  // - completely empty or banner-only inputs return empty string
  let raw_collapsing = "line 1\n\n\n   \n\t \nline 2\n\nline 3";
  assert_eq!(
    normalize_diagnostics(raw_collapsing),
    "line 1\n\nline 2\n\nline 3"
  );

  let raw_leading_trailing = "\n\n   \n\t\nline 1\nline 2\n\n   \n";
  assert_eq!(
    normalize_diagnostics(raw_leading_trailing),
    "line 1\nline 2"
  );

  assert_eq!(normalize_diagnostics(""), "");
  assert_eq!(normalize_diagnostics("   \n\t\n\n"), "");
  assert_eq!(
    normalize_diagnostics(
      "Checking formatting...\n   \n\nAll checks passed!\n"
    ),
    ""
  );
}

#[test]
fn test_execution_error_and_violations_render_detail_identically() {
  // Issue #146: identical raw tool output must produce byte-identical rendered
  // detail regardless of which status arm it lands in. This calls
  // `tool_output_detail` directly.
  //
  // Call-site wiring is verified separately through `collect_diagnostics` (issue #175).
  let raw = "Checking formatting...\n\nsrc/x.js: error   \n  2:1  Delete `;`\n\nAll checks passed!\nCommand failed with exit code 2\n";

  // ViolationsFound with no diff, and ExecutionError, both pass `diff: None`
  // through to `tool_output_detail`, so one call covers both arms' input.
  let exec_error_detail = tool_output_detail(raw, None);

  assert_eq!(
    exec_error_detail,
    "src/x.js: error\n  2:1  Delete `;`\n\nCommand failed with exit code 2"
  );
}

#[test]
fn test_tool_output_detail_renders_message_then_diff() {
  // A diff is still rendered verbatim (bypassing normalize_diagnostics,
  // whose blank-line trimming would eat diff *content*), but it no longer
  // replaces the message. `fml fix --check` folds a lint result and a
  // format result for one surface into a single status, so both halves are
  // routinely present at once and returning only the diff silently dropped
  // every lint finding.
  let detail = tool_output_detail(
    "Checking formatting...\nraw message noise",
    Some("- old\n+ new"),
  );
  assert_eq!(detail, "raw message noise\n- old\n+ new");
}

#[test]
fn test_tool_output_detail_diff_alone_when_message_is_empty() {
  // `diff_check_via_tempcopy_classified` is the only producer of a diff and
  // always pairs it with an empty message, so a plain `fml fmt --check`
  // must render exactly the diff with no leading blank line -- i.e. output
  // unchanged by message-then-diff rendering.
  let detail = tool_output_detail("", Some("- old\n+ new"));
  assert_eq!(detail, "- old\n+ new");
}

#[test]
fn test_tool_output_detail_message_alone_when_no_diff() {
  let detail = tool_output_detail("Checking formatting...\nreal finding", None);
  assert_eq!(detail, "real finding");
}

#[test]
fn test_collect_diagnostics_execution_error_arm_normalizes() {
  // Issue #146, #175: build a real ExecutionError surfaces::SurfaceResult with noisy raw
  // tool output and check the detail `collect_diagnostics` computes for it,
  // confirming the ExecutionError arm is wired to normalize diagnostics.
  let raw = "Checking formatting...\n\n  fatal: crashed   \n\nAll checks passed!\nCommand failed with exit code 2\n";
  let exec_result = surfaces::SurfaceResult {
    surface_name: "go",
    status: surfaces::SurfaceStatus::ExecutionError {
      message: raw.to_string(),
    },
    duration: time::Duration::from_millis(5),
  };

  let diags = collect_diagnostics(&[exec_result]);
  assert_eq!(diags.len(), 1);
  assert_eq!(diags[0].0, "go");
  assert_eq!(
    diags[0].1,
    "  fatal: crashed\n\nCommand failed with exit code 2"
  );
  assert!(!diags[0].1.contains("Checking formatting..."));
  assert!(!diags[0].1.contains("All checks passed!"));
}

#[test]
fn test_collect_diagnostics_execution_error_preserves_go_panic_goroutine_grouping()
 {
  // Issue #179: Go panics separate distinct goroutine stack traces with
  // blank lines. These must remain grouped and readable in ExecutionError
  // diagnostics rather than collapsing into an undifferentiated wall of frames.
  let raw = "\
Checking formatting...

panic: runtime error: index out of range [2] with length 1

goroutine 1 [running]:
main.main()
\t/app/main.go:8 +0x54

goroutine 2 [select]:
main.worker(0xc00008e000)
\t/app/worker.go:14 +0x22

Command failed with exit code 2
";
  let exec_result = surfaces::SurfaceResult {
    surface_name: "go",
    status: surfaces::SurfaceStatus::ExecutionError {
      message: raw.to_string(),
    },
    duration: time::Duration::from_millis(10),
  };

  let diags = collect_diagnostics(&[exec_result]);
  assert_eq!(diags.len(), 1);
  assert_eq!(diags[0].0, "go");
  let expected = "\
panic: runtime error: index out of range [2] with length 1

goroutine 1 [running]:
main.main()
\t/app/main.go:8 +0x54

goroutine 2 [select]:
main.worker(0xc00008e000)
\t/app/worker.go:14 +0x22

Command failed with exit code 2";
  assert_eq!(diags[0].1, expected);
}

#[test]
fn test_collect_diagnostics_violations_found_arm_normalizes() {
  // Issue #146, #175: build a real ViolationsFound surfaces::SurfaceResult with noisy raw
  // tool output and diff, confirming the ViolationsFound arm is wired to normalize
  // diagnostics and format diffs.
  let raw = "Checking formatting...\n\nsrc/x.js: error   \n  2:1  Delete `;`\n\nAll checks passed!\nCommand failed with exit code 2\n";
  let violations_result = surfaces::SurfaceResult {
    surface_name: "javascript",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: raw.to_string(),
      diff: Some("- old\n+ new".to_string()),
    },
    duration: time::Duration::from_millis(5),
  };

  let diags = collect_diagnostics(&[violations_result]);
  assert_eq!(diags.len(), 1);
  assert_eq!(diags[0].0, "javascript");
  assert_eq!(
    diags[0].1,
    "src/x.js: error\n  2:1  Delete `;`\n\nCommand failed with exit code 2\n- old\n+ new"
  );
  assert!(!diags[0].1.contains("Checking formatting..."));
  assert!(!diags[0].1.contains("All checks passed!"));
}

#[test]
fn test_collect_diagnostics_execution_error_and_violations_parity() {
  // Issue #146, #175: identical raw tool output must produce byte-identical
  // rendered detail regardless of whether it was classified as ViolationsFound
  // or ExecutionError.
  //
  // Un-wiring either arm (original asymmetry: ViolationsFound normalized but
  // ExecutionError not; reverse asymmetry: ExecutionError normalized but
  // ViolationsFound not) causes this parity assertion to fail.
  let raw = "Checking formatting...\n\nsrc/x.js: error   \n  2:1  Delete `;`\n\nAll checks passed!\nCommand failed with exit code 2\n";
  let violations_res = surfaces::SurfaceResult {
    surface_name: "js",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: raw.to_string(),
      diff: None,
    },
    duration: time::Duration::from_millis(5),
  };
  let exec_error_res = surfaces::SurfaceResult {
    surface_name: "js",
    status: surfaces::SurfaceStatus::ExecutionError {
      message: raw.to_string(),
    },
    duration: time::Duration::from_millis(5),
  };

  let violations_diags = collect_diagnostics(&[violations_res]);
  let exec_error_diags = collect_diagnostics(&[exec_error_res]);

  assert_eq!(violations_diags.len(), 1);
  assert_eq!(exec_error_diags.len(), 1);
  assert_eq!(violations_diags[0].0, "js");
  assert_eq!(exec_error_diags[0].0, "js");

  // Parity assertion: identical raw output produces byte-identical diagnostic detail.
  assert_eq!(violations_diags[0].1, exec_error_diags[0].1);
  assert_eq!(
    violations_diags[0].1,
    "src/x.js: error\n  2:1  Delete `;`\n\nCommand failed with exit code 2"
  );
}

#[test]
fn test_collect_diagnostics_all_statuses() {
  let results = vec![
    surfaces::SurfaceResult {
      surface_name: "clean",
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(1),
    },
    surfaces::SurfaceResult {
      surface_name: "synced",
      status: surfaces::SurfaceStatus::ConfigSynced {
        files: vec![surfaces::SyncedConfigFile::new(".prettierrc", true)],
      },
      duration: time::Duration::from_millis(1),
    },
    surfaces::SurfaceResult {
      surface_name: "skipped",
      status: surfaces::SurfaceStatus::Skipped {
        reason: "not installed".to_string(),
      },
      duration: time::Duration::from_millis(1),
    },
    surfaces::SurfaceResult {
      surface_name: "drifted",
      status: surfaces::SurfaceStatus::ConfigDrifted {
        file: ".rustfmt.toml".to_string(),
        diff: "- old\n+ new".to_string(),
      },
      duration: time::Duration::from_millis(1),
    },
    surfaces::SurfaceResult {
      surface_name: "manual",
      status: surfaces::SurfaceStatus::ManualConfig {
        file: "tsconfig.json".to_string(),
        suggestion: "Please update tsconfig.json manually".to_string(),
      },
      duration: time::Duration::from_millis(1),
    },
    surfaces::SurfaceResult {
      surface_name: "missing",
      status: surfaces::SurfaceStatus::ToolMissing {
        binary: "biome".to_string(),
        install_hint: "npm i -g @biomejs/biome".to_string(),
      },
      duration: time::Duration::from_millis(1),
    },
  ];

  let diags = collect_diagnostics(&results);
  assert_eq!(diags.len(), 3);
  assert_eq!(diags[0].0, "drifted");
  assert_eq!(
    diags[0].1,
    "Native config '.rustfmt.toml' drifted from formality.toml:\n- old\n+ new"
  );
  assert_eq!(diags[1].0, "manual");
  assert_eq!(diags[1].1, "Please update tsconfig.json manually");
  assert_eq!(diags[2].0, "missing");
  assert_eq!(
    diags[2].1,
    "Missing tool binary 'biome'.\n  Install hint: npm i -g @biomejs/biome"
  );
}

#[test]
fn test_runner_single_walk_polyglot_repo() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let fixture = manifest_dir.join("tests/fixtures/polyglot_repo");

  // Single candidate filesystem walk
  let candidates = surfaces::walk_candidate_files(&fixture, &[]);
  assert!(
    candidates.len() >= 7,
    "Expected at least 7 files in polyglot_repo, found {}",
    candidates.len()
  );

  // Filter in-memory for each surface
  let rust_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(&surfaces::rust::RustSurface),
    &[],
    &[],
  );
  assert_eq!(rust_files.len(), 1);
  assert!(rust_files[0].ends_with("main.rs"));

  let py_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(
      &surfaces::python::PythonSurface,
    ),
    &[],
    &[],
  );
  assert_eq!(py_files.len(), 1);
  assert!(py_files[0].ends_with("script.py"));

  let md_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(
      &surfaces::markdown::MarkdownSurface,
    ),
    &[],
    &[],
  );
  assert_eq!(md_files.len(), 1);
  assert!(md_files[0].ends_with("README.md"));

  let yaml_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(&surfaces::yaml::YamlSurface),
    &[],
    &[],
  );
  assert_eq!(yaml_files.len(), 1);
  assert!(yaml_files[0].ends_with("config.yaml"));

  let json_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(&surfaces::json::JsonSurface),
    &[],
    &[],
  );
  assert_eq!(json_files.len(), 1);
  assert!(json_files[0].ends_with("data.json"));

  let typst_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(&surfaces::typst::TypstSurface),
    &[],
    &[],
  );
  assert_eq!(typst_files.len(), 1);
  assert!(typst_files[0].ends_with("doc.typ"));

  let toml_files = surfaces::filter_candidates_with_ext(
    &candidates,
    surfaces::LanguageSurface::file_extensions(&surfaces::toml::TomlSurface),
    &[],
    &[],
  );
  assert_eq!(toml_files.len(), 1);
  assert!(toml_files[0].ends_with("Cargo.toml"));
}

#[test]
fn test_execution_context_candidate_files_filtering() {
  let candidates = sync::Arc::new(vec![
    path::PathBuf::from("/ws/src/main.rs"),
    path::PathBuf::from("/ws/src/lib.rs"),
    path::PathBuf::from("/ws/src/ignored.rs"),
    path::PathBuf::from("/ws/script.py"),
  ]);

  let mut lang_config = config::ResolvedLangConfig::new("rust");
  lang_config.exclude = vec![path::PathBuf::from("ignored.rs")];

  let ctx = surfaces::ExecutionContext {
    root: sync::Arc::new(path::PathBuf::from("/ws")),
    paths: sync::Arc::new(Vec::new()),
    global_config: sync::Arc::new(config::ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: candidates,
  };

  let matched = ctx.matched_files(&["rs"]);
  assert_eq!(matched.len(), 2);
  assert!(matched.contains(&path::PathBuf::from("/ws/src/main.rs")));
  assert!(matched.contains(&path::PathBuf::from("/ws/src/lib.rs")));
  assert!(!matched.contains(&path::PathBuf::from("/ws/src/ignored.rs")));
  assert!(!matched.contains(&path::PathBuf::from("/ws/script.py")));
}

#[test]
fn test_execution_context_staged_files_filtering() {
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();

  let src = root.join("src");
  let fixtures = root.join("fixtures");
  std::fs::create_dir_all(&src).unwrap();
  std::fs::create_dir_all(&fixtures).unwrap();

  let main_rs = src.join("main.rs");
  let excluded_rs = src.join("generated.rs");
  let fixture_rs = fixtures.join("mock.rs");
  let py_file = root.join("script.py");

  std::fs::write(&main_rs, "fn main() {}\n").unwrap();
  std::fs::write(&excluded_rs, "fn gen() {}\n").unwrap();
  std::fs::write(&fixture_rs, "fn mock() {}\n").unwrap();
  std::fs::write(&py_file, "print('hi')\n").unwrap();

  let staged_paths =
    sync::Arc::new(vec![main_rs.clone(), excluded_rs, fixture_rs, py_file]);

  let mut lang_config = config::ResolvedLangConfig::new("rust");
  lang_config.exclude = vec![path::PathBuf::from("src/generated.rs")];

  let ctx = surfaces::ExecutionContext {
    root: sync::Arc::new(root.to_path_buf()),
    paths: sync::Arc::clone(&staged_paths),
    global_config: sync::Arc::new(config::ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: sync::Arc::new(surfaces::glob::expand_targets(
      root,
      &staged_paths,
    )),
  };

  let matched = ctx.matched_files(&["rs"]);
  assert_eq!(matched, vec![main_rs]);
}

#[test]
fn test_passed_detail_reads_as_already_in_sync_for_sync() {
  // Issue #130: a `fml sync` no-op rendered `Clean / Formatted`, which is the
  // wrong vocabulary — nothing was formatted, the config file simply already
  // matched formality.toml.
  assert_eq!(passed_detail(&Plan::sync(false)), "Already in sync");
  assert_eq!(passed_detail(&Plan::sync(true)), "Already in sync");
  assert_eq!(passed_detail(&Plan::fmt(false, false)), "Clean / Formatted");
  assert_eq!(passed_detail(&Plan::lint(false)), "Clean / Formatted");
  assert_eq!(passed_detail(&Plan::fix(false, false)), "Clean / Formatted");
}

#[test]
fn test_header_count_label_pluralizes_on_the_row_count() {
  // Issue #130: the count is the number of rendered rows, not the number of
  // matched surfaces — `fml sync` appends shared-config rows after the fan-out.
  assert_eq!(header_count_label(1), "1 surface");
  assert_eq!(header_count_label(2), "2 surfaces");
  assert_eq!(header_count_label(0), "0 surfaces");
}

#[test]
fn test_synced_files_detail_names_every_file() {
  assert_eq!(
    synced_files_detail(&[
      surfaces::SyncedConfigFile::new(".clang-format", true),
      surfaces::SyncedConfigFile::new(".clang-tidy", false),
    ]),
    "Created .clang-format, Synced .clang-tidy"
  );
  assert_eq!(
    synced_files_detail(&[surfaces::SyncedConfigFile::new(
      ".rustfmt.toml",
      true
    )]),
    "Created .rustfmt.toml"
  );
}

#[derive(Debug, Clone)]
struct MockMissingSurface;

impl surfaces::DeclaresFacets for MockMissingSurface {
  fn facet_support(&self, _: surfaces::Facet) -> surfaces::FacetSupport {
    surfaces::FacetSupport::Unsupported
  }
}

impl surfaces::LanguageSurface for MockMissingSurface {
  fn name(&self) -> &'static str {
    "mock_missing"
  }
  fn file_extensions(&self) -> &[&'static str] {
    &["mock"]
  }
  fn tool_info(
    &self,
    _: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![]
  }
  fn format(&self, _: &surfaces::ExecutionContext) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::ToolMissing {
        binary: "mock-tool".to_string(),
        install_hint: "echo install".to_string(),
      },
      duration: time::Duration::from_millis(1),
    }
  }
  fn lint(
    &self,
    _: &surfaces::ExecutionContext,
    _: bool,
  ) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::ToolMissing {
        binary: "mock-tool".to_string(),
        install_hint: "echo install".to_string(),
      },
      duration: time::Duration::from_millis(1),
    }
  }
  fn sync_config(
    &self,
    _: &surfaces::ExecutionContext,
    _: bool,
  ) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(1),
    }
  }
  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(self.clone())
  }
}

#[test]
fn test_runner_missing_tool_exit_code_is_violations() {
  // #252: a missing tool is an unmet precondition, not a clean run — it must
  // not let the process exit 0. It is also not `ExitStatus::Error`: the tool
  // correctly determined it could not proceed, which is the same severity as
  // a real violation, not an operational fault. Exercised across `lint` and
  // `fmt`, staged and unstaged, and both `--check`/write forms, since the
  // fix is unconditional on mode.
  let root = path::PathBuf::from(".");
  let config = config::FormalityConfig::default();
  let staged_paths =
    Scope::resolve(&root, &[path::PathBuf::from("test.mock")], &[]);

  // Lint unstaged & staged
  let unstaged_lint = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::lint(false),
    &config,
  );
  assert_eq!(unstaged_lint, errors::ExitStatus::Violations);

  let staged_lint = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &staged_paths,
    &Plan::lint(false),
    &config,
  );
  assert_eq!(staged_lint, errors::ExitStatus::Violations);
  assert_eq!(unstaged_lint, staged_lint);

  // Fmt unstaged & staged, write mode
  let unstaged_fmt = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fmt(false, false),
    &config,
  );
  assert_eq!(unstaged_fmt, errors::ExitStatus::Violations);

  let staged_fmt = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &staged_paths,
    &Plan::fmt(false, false),
    &config,
  );
  assert_eq!(staged_fmt, errors::ExitStatus::Violations);
  assert_eq!(unstaged_fmt, staged_fmt);

  // Fmt --check (Mode::Report) — the fix is unconditional on mode, so this
  // must also be non-zero, not just the write form above.
  let check_fmt = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fmt(true, false),
    &config,
  );
  assert_eq!(check_fmt, errors::ExitStatus::Violations);

  // Fix (Lint + Format) — a plan neither prior case exercises directly.
  let fix = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fix(false, false),
    &config,
  );
  assert_eq!(fix, errors::ExitStatus::Violations);
}

/// A surface whose format/lint pass always reports a real violation --
/// distinct from [`MockMissingSurface`], which reports `ToolMissing`.
#[derive(Debug, Clone)]
struct MockViolatingSurface;

impl surfaces::DeclaresFacets for MockViolatingSurface {
  fn facet_support(&self, _: surfaces::Facet) -> surfaces::FacetSupport {
    surfaces::FacetSupport::Unsupported
  }
}

impl surfaces::LanguageSurface for MockViolatingSurface {
  fn name(&self) -> &'static str {
    "mock_violating"
  }
  fn file_extensions(&self) -> &[&'static str] {
    &["mock2"]
  }
  fn tool_info(
    &self,
    _: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![]
  }
  fn format(&self, _: &surfaces::ExecutionContext) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "unformatted".to_string(),
        diff: None,
      },
      duration: time::Duration::from_millis(1),
    }
  }
  fn lint(
    &self,
    _: &surfaces::ExecutionContext,
    _: bool,
  ) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::ViolationsFound {
        message: "lint violation".to_string(),
        diff: None,
      },
      duration: time::Duration::from_millis(1),
    }
  }
  fn sync_config(
    &self,
    _: &surfaces::ExecutionContext,
    _: bool,
  ) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(1),
    }
  }
  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(self.clone())
  }
}

/// A surface whose format/lint pass always reports an operational fault --
/// distinct from [`MockMissingSurface`] (`ToolMissing`) and
/// [`MockViolatingSurface`] (`ViolationsFound`).
#[derive(Debug, Clone)]
struct MockErroringSurface;

impl surfaces::DeclaresFacets for MockErroringSurface {
  fn facet_support(&self, _: surfaces::Facet) -> surfaces::FacetSupport {
    surfaces::FacetSupport::Unsupported
  }
}

impl surfaces::LanguageSurface for MockErroringSurface {
  fn name(&self) -> &'static str {
    "mock_erroring"
  }
  fn file_extensions(&self) -> &[&'static str] {
    &["mock3"]
  }
  fn tool_info(
    &self,
    _: &config::ResolvedLangConfig,
  ) -> Vec<surfaces::ToolInfo> {
    vec![]
  }
  fn format(&self, _: &surfaces::ExecutionContext) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::ExecutionError {
        message: "tool crashed".to_string(),
      },
      duration: time::Duration::from_millis(1),
    }
  }
  fn lint(
    &self,
    _: &surfaces::ExecutionContext,
    _: bool,
  ) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::ExecutionError {
        message: "tool crashed".to_string(),
      },
      duration: time::Duration::from_millis(1),
    }
  }
  fn sync_config(
    &self,
    _: &surfaces::ExecutionContext,
    _: bool,
  ) -> surfaces::SurfaceResult {
    surfaces::SurfaceResult {
      surface_name: self.name(),
      status: surfaces::SurfaceStatus::Passed,
      duration: time::Duration::from_millis(1),
    }
  }
  fn clone_box(&self) -> Box<dyn surfaces::LanguageSurface> {
    Box::new(self.clone())
  }
}

#[test]
fn test_runner_allow_missing_silences_tool_missing_but_not_violations() {
  // #252 / #163: `--allow-missing` must silence a missing tool's
  // contribution to the exit code (restoring #163's guarantee) while
  // leaving a real violation elsewhere fatal, and leaving the missing-tool
  // row itself visible either way (the whole point is not recreating the
  // original silent-pass bug).
  let root = path::PathBuf::from(".");
  let config = config::FormalityConfig::default();

  // A missing tool alone: exit 1 without --allow-missing.
  let without_flag = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fmt(false, false),
    &config,
  );
  assert_eq!(without_flag, errors::ExitStatus::Violations);

  // ...and exit 0 with --allow-missing.
  let with_flag = Runner::run(
    &[Box::new(MockMissingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fmt(false, true),
    &config,
  );
  assert_eq!(with_flag, errors::ExitStatus::Clean);

  // A missing tool AND a real violation (on a different surface): still
  // exit 1 even with --allow-missing -- the flag only silences the
  // ToolMissing arm, never a genuine violation.
  let missing_and_violating = Runner::run(
    &[Box::new(MockMissingSurface), Box::new(MockViolatingSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fmt(false, true),
    &config,
  );
  assert_eq!(missing_and_violating, errors::ExitStatus::Violations);

  // A missing tool AND an execution error (on a different surface): exit 2
  // even with --allow-missing -- the flag only silences the ToolMissing arm,
  // never an operational fault, which outranks a plain violation too.
  let missing_and_erroring = Runner::run(
    &[Box::new(MockMissingSurface), Box::new(MockErroringSurface)],
    &root,
    &Scope::Workspace(sync::Arc::default()),
    &Plan::fmt(false, true),
    &config,
  );
  assert_eq!(missing_and_erroring, errors::ExitStatus::Error);
}

#[test]
fn test_combine_pass_results_violations_over_tool_missing() {
  let lint_res = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::ToolMissing {
      binary: "markdownlint-cli2".to_string(),
      install_hint: "npm install -g markdownlint-cli2".to_string(),
    },
    duration: time::Duration::from_millis(10),
  };
  let fmt_res = surfaces::SurfaceResult {
    surface_name: "markdown",
    status: surfaces::SurfaceStatus::ViolationsFound {
      message: "unformatted".to_string(),
      diff: Some("diff".to_string()),
    },
    duration: time::Duration::from_millis(20),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    surfaces::SurfaceStatus::ViolationsFound { .. }
  ));
  assert_eq!(combined.duration, time::Duration::from_millis(30));
}
