use super::*;
use crate::surfaces::ToolInfo;

#[test]
fn test_tool_info_auto_install_cmd_coverage() {
  let tools = [
    "taplo",
    "typstyle",
    "tinymist",
    "ruff",
    "prettier",
    "biome",
    "markdownlint-cli2",
    "yamllint",
    "clang-format",
    "clang-tidy",
    "google-java-format",
    "checkstyle",
    "rustfmt",
    "clippy-driver",
    "goimports",
    "golangci-lint",
    "ktlint",
  ];

  for binary in tools {
    let info = ToolInfo {
      binary,
      description: "test tool",
      install_hint: "test hint",
      is_required_for_fmt: true,
      is_required_for_lint: true,
    };

    // Ensure get_auto_install_cmd executes without error
    let cmd = info.get_auto_install_cmd();
    if let Some((program, args)) = cmd {
      assert!(!program.is_empty());
      assert!(!args.is_empty());
    }
  }
}

#[test]
fn test_unknown_tool_has_no_install_chain() {
  let info = ToolInfo {
    binary: "not-a-real-tool",
    description: "test tool",
    install_hint: "test hint",
    is_required_for_fmt: false,
    is_required_for_lint: false,
  };
  assert!(info.get_auto_install_cmd().is_none());
}

// Command-shape tests below are pure and environment-independent: they
// exercise InstallMethod::command() directly rather than going through
// is_available(), so they don't depend on what's actually installed on
// the machine running the tests.

#[test]
fn test_install_method_command_shapes() {
  assert_eq!(
    InstallMethod::CargoBinstall("ruff").command(),
    (
      "cargo".to_string(),
      vec!["binstall".to_string(), "-y".to_string(), "ruff".to_string()]
    )
  );
  assert_eq!(
    InstallMethod::Npm("@taplo/cli").command(),
    (
      "npm".to_string(),
      vec![
        "install".to_string(),
        "-g".to_string(),
        "@taplo/cli".to_string()
      ]
    )
  );
  assert_eq!(
    InstallMethod::Cargo {
      package: "typstyle",
      locked: true
    }
    .command(),
    (
      "cargo".to_string(),
      vec![
        "install".to_string(),
        "typstyle".to_string(),
        "--locked".to_string()
      ]
    )
  );
  assert_eq!(
    InstallMethod::Cargo {
      package: "some-tool",
      locked: false
    }
    .command(),
    (
      "cargo".to_string(),
      vec!["install".to_string(), "some-tool".to_string()]
    )
  );
  assert_eq!(
    InstallMethod::WingetId("tamasfe.taplo").command(),
    (
      "winget".to_string(),
      vec![
        "install".to_string(),
        "--id=tamasfe.taplo".to_string(),
        "-e".to_string(),
        "--accept-source-agreements".to_string(),
        "--accept-package-agreements".to_string(),
      ]
    )
  );
  assert_eq!(
    InstallMethod::WingetName("LLVM.LLVM").command(),
    (
      "winget".to_string(),
      vec![
        "install".to_string(),
        "LLVM.LLVM".to_string(),
        "--accept-source-agreements".to_string(),
        "--accept-package-agreements".to_string(),
      ]
    )
  );
  assert_eq!(
    InstallMethod::Rustup("clippy").command(),
    (
      "rustup".to_string(),
      vec![
        "component".to_string(),
        "add".to_string(),
        "clippy".to_string()
      ]
    )
  );
  let (prog, args) = InstallMethod::Apt("clang-format").command();
  assert!(prog == "sudo" || prog == "apt-get");
  assert!(args.contains(&"clang-format".to_string()));
}

#[test]
fn test_extra_args_wired_to_command() {
  let mut cmd = create_tool_command("cargo");
  let extra_args = vec!["--verbose".to_string(), "--locked".to_string()];
  cmd.args(&extra_args);
  let args: Vec<String> = cmd
    .get_args()
    .map(|a| a.to_string_lossy().to_string())
    .collect();
  assert!(args.contains(&"--verbose".to_string()));
  assert!(args.contains(&"--locked".to_string()));
}

#[test]
fn test_check_binary_exists_caching() {
  let non_existent = "non_existent_binary_xyz_12345";
  let non_existent_result = check_binary_exists(non_existent);
  assert!(!non_existent_result);

  let existing = "cargo";
  let existing_result = check_binary_exists(existing);

  // Inspect BINARY_CACHE directly to verify process-lifetime memoization
  let cache = BINARY_CACHE.get().expect("cache should be initialized");
  let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
  assert_eq!(guard.get(non_existent), Some(&false));
  assert_eq!(guard.get(existing), Some(&existing_result));
}

#[test]
fn test_check_binary_exists_thread_safety() {
  let handles: Vec<_> = (0..10)
    .map(|i| {
      std::thread::spawn(move || {
        let binary_name = format!("thread_test_binary_{i}");
        for _ in 0..50 {
          let _ = check_binary_exists("cargo");
          let _ = check_binary_exists(&binary_name);
        }
      })
    })
    .collect();

  for handle in handles {
    handle.join().unwrap();
  }

  let cache = BINARY_CACHE.get().expect("cache should be initialized");
  let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
  assert!(guard.contains_key("cargo"));
  for i in 0..10 {
    let binary_name = format!("thread_test_binary_{i}");
    assert!(guard.contains_key(&binary_name));
  }
}

