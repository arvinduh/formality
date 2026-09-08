use super::*;
use std::time::Duration;

#[test]
fn test_combine_pass_results_passed_and_skipped() {
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

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert_eq!(combined.surface_name, "yaml");
  assert_eq!(combined.duration, Duration::from_millis(30));
  assert!(matches!(combined.status, SurfaceStatus::Passed));
}

#[test]
fn test_combine_pass_results_both_passed() {
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

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert_eq!(combined.surface_name, "python");
  assert_eq!(combined.duration, Duration::from_millis(40));
  assert!(matches!(combined.status, SurfaceStatus::Passed));
}

#[test]
fn test_combine_pass_results_recheck_clears_lint_violation() {
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

  let combined =
    combine_pass_results(apply_recheck(lint_res, Some(recheck)), fmt_res);
  assert!(matches!(combined.status, SurfaceStatus::Passed));
  assert_eq!(combined.duration, Duration::from_millis(90));
}

#[test]
fn test_combine_pass_results_recheck_preserves_surviving_violation() {
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

  let combined =
    combine_pass_results(apply_recheck(lint_res, Some(recheck)), fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ViolationsFound { message, .. }
      if message.contains("MD025")
  ));
  assert_eq!(combined.duration, Duration::from_millis(90));
}

#[test]
fn test_combine_pass_results_violations_precedence() {
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

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ViolationsFound { message, .. } if message.contains("warning: unused")
  ));
}

#[test]
fn test_combine_pass_results_tool_missing_precedence() {
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

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ToolMissing { binary, .. } if binary == "ruff"
  ));
}

#[test]
fn test_combine_pass_results_execution_error_precedence() {
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

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
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
  // `tool_output_detail` directly.
  //
  // Call-site wiring is verified separately through `collect_diagnostics` (issue #175).
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
  // Issue #146, #175: build a real ExecutionError SurfaceResult with noisy raw
  // tool output and check the detail `collect_diagnostics` computes for it,
  // confirming the ExecutionError arm is wired to normalize diagnostics.
  let raw = "Checking formatting...\n\n  fatal: crashed   \n\nAll checks passed!\nCommand failed with exit code 2\n";
  let exec_result = SurfaceResult {
    surface_name: "go",
    status: SurfaceStatus::ExecutionError {
      message: raw.to_string(),
    },
    duration: Duration::from_millis(5),
  };

  let diags = collect_diagnostics(&[exec_result]);
  assert_eq!(diags.len(), 1);
  assert_eq!(diags[0].0, "go");
  assert_eq!(
    diags[0].1,
    "  fatal: crashed\nCommand failed with exit code 2"
  );
  assert!(!diags[0].1.contains("Checking formatting..."));
  assert!(!diags[0].1.contains("All checks passed!"));
}

#[test]
fn test_collect_diagnostics_violations_found_arm_normalizes() {
  // Issue #146, #175: build a real ViolationsFound SurfaceResult with noisy raw
  // tool output and diff, confirming the ViolationsFound arm is wired to normalize
  // diagnostics and format diffs.
  let raw = "Checking formatting...\n\nsrc/x.js: error   \n  2:1  Delete `;`\n\nAll checks passed!\nCommand failed with exit code 2\n";
  let violations_result = SurfaceResult {
    surface_name: "javascript",
    status: SurfaceStatus::ViolationsFound {
      message: raw.to_string(),
      diff: Some("- old\n+ new".to_string()),
    },
    duration: Duration::from_millis(5),
  };

  let diags = collect_diagnostics(&[violations_result]);
  assert_eq!(diags.len(), 1);
  assert_eq!(diags[0].0, "javascript");
  assert_eq!(
    diags[0].1,
    "src/x.js: error\n  2:1  Delete `;`\nCommand failed with exit code 2\n- old\n+ new"
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
  let violations_res = SurfaceResult {
    surface_name: "js",
    status: SurfaceStatus::ViolationsFound {
      message: raw.to_string(),
      diff: None,
    },
    duration: Duration::from_millis(5),
  };
  let exec_error_res = SurfaceResult {
    surface_name: "js",
    status: SurfaceStatus::ExecutionError {
      message: raw.to_string(),
    },
    duration: Duration::from_millis(5),
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
    "src/x.js: error\n  2:1  Delete `;`\nCommand failed with exit code 2"
  );
}

#[test]
fn test_collect_diagnostics_all_statuses() {
  use crate::surfaces::SyncedConfigFile;

  let results = vec![
    SurfaceResult {
      surface_name: "clean",
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(1),
    },
    SurfaceResult {
      surface_name: "synced",
      status: SurfaceStatus::ConfigSynced {
        files: vec![SyncedConfigFile::new(".prettierrc", true)],
      },
      duration: Duration::from_millis(1),
    },
    SurfaceResult {
      surface_name: "skipped",
      status: SurfaceStatus::Skipped {
        reason: "not installed".to_string(),
      },
      duration: Duration::from_millis(1),
    },
    SurfaceResult {
      surface_name: "drifted",
      status: SurfaceStatus::ConfigDrifted {
        file: ".rustfmt.toml".to_string(),
        diff: "- old\n+ new".to_string(),
      },
      duration: Duration::from_millis(1),
    },
    SurfaceResult {
      surface_name: "manual",
      status: SurfaceStatus::ManualConfig {
        file: "tsconfig.json".to_string(),
        suggestion: "Please update tsconfig.json manually".to_string(),
      },
      duration: Duration::from_millis(1),
    },
    SurfaceResult {
      surface_name: "missing",
      status: SurfaceStatus::ToolMissing {
        binary: "biome".to_string(),
        install_hint: "npm i -g @biomejs/biome".to_string(),
      },
      duration: Duration::from_millis(1),
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

  let staged_paths = Arc::new(vec![
    main_rs.clone(),
    excluded_rs.clone(),
    fixture_rs.clone(),
    py_file.clone(),
  ]);

  let mut lang_config = crate::config::ResolvedLangConfig::new("rust");
  lang_config.exclude = vec![PathBuf::from("src/generated.rs")];

  let ctx = ExecutionContext {
    root: Arc::new(root.to_path_buf()),
    paths: staged_paths,
    global_config: Arc::new(crate::config::ResolvedGlobalConfig::default()),
    lang_config,
    check_only: false,
    candidate_files: None,
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
  assert_eq!(passed_detail(&Plan::fmt(false)), "Clean / Formatted");
  assert_eq!(passed_detail(&Plan::lint()), "Clean / Formatted");
  assert_eq!(passed_detail(&Plan::fix(false)), "Clean / Formatted");
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
  use crate::surfaces::SyncedConfigFile;
  assert_eq!(
    synced_files_detail(&[
      SyncedConfigFile::new(".clang-format", true),
      SyncedConfigFile::new(".clang-tidy", false),
    ]),
    "Created .clang-format, Synced .clang-tidy"
  );
  assert_eq!(
    synced_files_detail(&[SyncedConfigFile::new(".rustfmt.toml", true)]),
    "Created .rustfmt.toml"
  );
}

#[derive(Debug, Clone)]
struct MockMissingSurface;

impl crate::surfaces::DeclaresFacets for MockMissingSurface {
  fn facet_support(
    &self,
    _: crate::surfaces::Facet,
  ) -> crate::surfaces::FacetSupport {
    crate::surfaces::FacetSupport::Unsupported
  }
}

impl LanguageSurface for MockMissingSurface {
  fn name(&self) -> &'static str {
    "mock_missing"
  }
  fn file_extensions(&self) -> &[&'static str] {
    &["mock"]
  }
  fn detect(&self, _: &Path) -> bool {
    true
  }
  fn tool_info(
    &self,
    _: &crate::config::ResolvedLangConfig,
  ) -> Vec<crate::surfaces::ToolInfo> {
    vec![]
  }
  fn format(&self, _: &ExecutionContext) -> SurfaceResult {
    SurfaceResult {
      surface_name: self.name(),
      status: SurfaceStatus::ToolMissing {
        binary: "mock-tool".to_string(),
        install_hint: "echo install".to_string(),
      },
      duration: Duration::from_millis(1),
    }
  }
  fn lint(&self, _: &ExecutionContext, _: bool) -> SurfaceResult {
    SurfaceResult {
      surface_name: self.name(),
      status: SurfaceStatus::ToolMissing {
        binary: "mock-tool".to_string(),
        install_hint: "echo install".to_string(),
      },
      duration: Duration::from_millis(1),
    }
  }
  fn sync_config(&self, _: &ExecutionContext, _: bool) -> SurfaceResult {
    SurfaceResult {
      surface_name: self.name(),
      status: SurfaceStatus::Passed,
      duration: Duration::from_millis(1),
    }
  }
  fn clone_box(&self) -> Box<dyn LanguageSurface> {
    Box::new(self.clone())
  }
}

#[test]
fn test_runner_missing_tool_exit_code_is_clean() {
  let root = PathBuf::from(".");
  let config = FormalityConfig::default();
  let staged_paths = vec![PathBuf::from("test.mock")];

  // Lint unstaged & staged
  let unstaged_lint = Runner::run(
    vec![Box::new(MockMissingSurface)],
    &root,
    &[],
    &Plan::lint(),
    &config,
  );
  assert_eq!(unstaged_lint, ExitStatus::Clean);

  let staged_lint = Runner::run(
    vec![Box::new(MockMissingSurface)],
    &root,
    &staged_paths,
    &Plan::lint(),
    &config,
  );
  assert_eq!(staged_lint, ExitStatus::Clean);
  assert_eq!(unstaged_lint, staged_lint);

  // Fmt unstaged & staged
  let unstaged_fmt = Runner::run(
    vec![Box::new(MockMissingSurface)],
    &root,
    &[],
    &Plan::fmt(false),
    &config,
  );
  assert_eq!(unstaged_fmt, ExitStatus::Clean);

  let staged_fmt = Runner::run(
    vec![Box::new(MockMissingSurface)],
    &root,
    &staged_paths,
    &Plan::fmt(false),
    &config,
  );
  assert_eq!(staged_fmt, ExitStatus::Clean);
  assert_eq!(unstaged_fmt, staged_fmt);
}

#[test]
fn test_combine_pass_results_violations_over_tool_missing() {
  let lint_res = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::ToolMissing {
      binary: "markdownlint-cli2".to_string(),
      install_hint: "npm install -g markdownlint-cli2".to_string(),
    },
    duration: Duration::from_millis(10),
  };
  let fmt_res = SurfaceResult {
    surface_name: "markdown",
    status: SurfaceStatus::ViolationsFound {
      message: "unformatted".to_string(),
      diff: Some("diff".to_string()),
    },
    duration: Duration::from_millis(20),
  };

  let combined = combine_pass_results(apply_recheck(lint_res, None), fmt_res);
  assert!(matches!(
    combined.status,
    SurfaceStatus::ViolationsFound { .. }
  ));
  assert_eq!(combined.duration, Duration::from_millis(30));
}
