use super::*;

#[test]
fn test_version_constructors_and_display() {
  let v1 = Version::new(1, 4, 0);
  assert_eq!(v1.to_string(), "1.4.0");
  assert_eq!(v1.major, 1);
  assert_eq!(v1.minor, 4);
  assert_eq!(v1.patch, 0);
  assert!(v1.prerelease.is_none());

  let v2 = Version::with_prerelease(1, 7, 0, "nightly");
  assert_eq!(v2.to_string(), "1.7.0-nightly");
  assert_eq!(v2.prerelease.as_deref(), Some("nightly"));
}

#[test]
fn test_version_parsing_direct() {
  assert_eq!(Version::parse("1.4.0"), Some(Version::new(1, 4, 0)));
  assert_eq!(Version::parse("v0.17.2"), Some(Version::new(0, 17, 2)));
  assert_eq!(Version::parse("V18.1.8"), Some(Version::new(18, 1, 8)));
  assert_eq!(Version::parse("1.4"), Some(Version::new(1, 4, 0)));
  assert_eq!(
    Version::parse("1.7.0-nightly"),
    Some(Version::with_prerelease(1, 7, 0, "nightly"))
  );
  assert_eq!(
    Version::parse("1.0.0-beta.2+20230101"),
    Some(Version::with_prerelease(1, 0, 0, "beta.2"))
  );
  assert_eq!(Version::parse(""), None);
  assert_eq!(Version::parse("invalid"), None);
}

#[test]
fn test_version_extraction_from_tool_banners() {
  let rustfmt = "rustfmt 1.7.0-nightly (7576e26b 2024-05-07)";
  assert_eq!(
    Version::extract(rustfmt),
    Some(Version::with_prerelease(1, 7, 0, "nightly"))
  );

  let ruff = "ruff 0.9.6";
  assert_eq!(Version::extract(ruff), Some(Version::new(0, 9, 6)));

  let clang_fmt = "clang-format version 18.1.8";
  assert_eq!(Version::extract(clang_fmt), Some(Version::new(18, 1, 8)));

  let clang_tidy = "clang-tidy version 14.0.0-1ubuntu1";
  assert_eq!(
    Version::extract(clang_tidy),
    Some(Version::with_prerelease(14, 0, 0, "1ubuntu1"))
  );

  let prettier = "prettier 3.5.1";
  assert_eq!(Version::extract(prettier), Some(Version::new(3, 5, 1)));

  let taplo = "taplo 0.9.3";
  assert_eq!(Version::extract(taplo), Some(Version::new(0, 9, 3)));

  let typstyle = "typstyle 0.12.0";
  assert_eq!(Version::extract(typstyle), Some(Version::new(0, 12, 0)));

  let markdownlint_cli2 = "markdownlint-cli2 v0.17.2 (markdownlint v0.37.0)";
  assert_eq!(
    Version::extract(markdownlint_cli2),
    Some(Version::new(0, 17, 2))
  );

  let clippy = "clippy 0.1.65 (rustc 1.65.0)";
  assert_eq!(Version::extract(clippy), Some(Version::new(0, 1, 65)));

  let yamllint = "yamllint 1.35.1";
  assert_eq!(Version::extract(yamllint), Some(Version::new(1, 35, 1)));

  let biome = "1.9.4";
  assert_eq!(Version::extract(biome), Some(Version::new(1, 9, 4)));

  let checkstyle = "Checkstyle version: 10.14.0";
  assert_eq!(Version::extract(checkstyle), Some(Version::new(10, 14, 0)));

  let checkstyle2 = "Checkstyle version 10.0.0";
  assert_eq!(Version::extract(checkstyle2), Some(Version::new(10, 0, 0)));

  let ktfmt = "ktfmt version 0.44";
  assert_eq!(Version::extract(ktfmt), Some(Version::new(0, 44, 0)));

  let ktlint = "1.0.1";
  assert_eq!(Version::extract(ktlint), Some(Version::new(1, 0, 1)));

  let go = "go version go1.21.5 darwin/arm64";
  assert_eq!(Version::extract(go), Some(Version::new(1, 21, 5)));

  let go_simple = "go1.18.0";
  assert_eq!(Version::extract(go_simple), Some(Version::new(1, 18, 0)));

  let golangci = "golangci-lint has version 1.55.2 built with go1.21.5 from 39c1b3f on 2023-12-04T12:00:00Z";
  assert_eq!(Version::extract(golangci), Some(Version::new(1, 55, 2)));
}

#[test]
fn test_version_ordering() {
  let v1_4_0 = Version::new(1, 4, 0);
  let v1_4_1 = Version::new(1, 4, 1);
  let v1_5_0 = Version::new(1, 5, 0);
  let v2_0_0 = Version::new(2, 0, 0);

  assert!(v1_4_0 < v1_4_1);
  assert!(v1_4_1 < v1_5_0);
  assert!(v1_5_0 < v2_0_0);
  assert!(v1_4_0 <= v1_4_0);
  assert!(v1_4_0 == v1_4_0);

  let v1_0_0 = Version::new(1, 0, 0);
  let v1_0_0_alpha = Version::with_prerelease(1, 0, 0, "alpha");
  let v1_0_0_alpha_1 = Version::with_prerelease(1, 0, 0, "alpha.1");
  let v1_0_0_alpha_beta = Version::with_prerelease(1, 0, 0, "alpha.beta");
  let v1_0_0_beta = Version::with_prerelease(1, 0, 0, "beta");
  let v1_0_0_beta_2 = Version::with_prerelease(1, 0, 0, "beta.2");
  let v1_0_0_beta_11 = Version::with_prerelease(1, 0, 0, "beta.11");
  let v1_0_0_rc_1 = Version::with_prerelease(1, 0, 0, "rc.1");

  // SemVer 2.0.0 Section 11 Specification ordering chain:
  // 1.0.0-alpha < 1.0.0-alpha.1 < 1.0.0-alpha.beta < 1.0.0-beta < 1.0.0-beta.2 < 1.0.0-beta.11 < 1.0.0-rc.1 < 1.0.0
  assert!(v1_0_0_alpha < v1_0_0_alpha_1);
  assert!(v1_0_0_alpha_1 < v1_0_0_alpha_beta);
  assert!(v1_0_0_alpha_beta < v1_0_0_beta);
  assert!(v1_0_0_beta < v1_0_0_beta_2);
  assert!(v1_0_0_beta_2 < v1_0_0_beta_11);
  assert!(v1_0_0_beta_11 < v1_0_0_rc_1);
  assert!(v1_0_0_rc_1 < v1_0_0);

  // Higher major/minor with prerelease is still greater than lower version
  let v1_7_0_nightly = Version::with_prerelease(1, 7, 0, "nightly");
  assert!(v1_7_0_nightly > v1_4_0);
}

#[test]
fn test_mstv_fleet_declarations() {
  assert_eq!(
    minimum_supported_tool_version("rustfmt"),
    Some(Version::new(1, 4, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("clippy"),
    Some(Version::new(1, 65, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("clippy-driver"),
    Some(Version::new(1, 65, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("cargo-clippy"),
    Some(Version::new(1, 65, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("ruff"),
    Some(Version::new(0, 1, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("clang-format"),
    Some(Version::new(14, 0, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("clang-tidy"),
    Some(Version::new(14, 0, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("prettier"),
    Some(Version::new(2, 0, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("taplo"),
    Some(Version::new(0, 8, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("markdownlint-cli2"),
    Some(Version::new(0, 4, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("typstyle"),
    Some(Version::new(0, 11, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("yamllint"),
    Some(Version::new(1, 20, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("biome"),
    Some(Version::new(1, 5, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("checkstyle"),
    Some(Version::new(10, 0, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("ktfmt"),
    Some(Version::new(0, 44, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("ktlint"),
    Some(Version::new(1, 0, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("gofmt"),
    Some(Version::new(1, 18, 0))
  );
  assert_eq!(
    minimum_supported_tool_version("golangci-lint"),
    Some(Version::new(1, 50, 0))
  );
  assert_eq!(minimum_supported_tool_version("unknown-tool"), None);

  assert_eq!(get_mstv("rustfmt"), Some(Version::new(1, 4, 0)));
  assert_eq!(get_mstv("yamllint"), Some(Version::new(1, 20, 0)));
  assert_eq!(get_mstv("typstyle"), Some(Version::new(0, 11, 0)));
  assert_eq!(get_mstv("biome"), Some(Version::new(1, 5, 0)));
  assert_eq!(get_mstv("checkstyle"), Some(Version::new(10, 0, 0)));
  assert_eq!(get_mstv("ktfmt"), Some(Version::new(0, 44, 0)));
  assert_eq!(get_mstv("ktlint"), Some(Version::new(1, 0, 0)));
  assert_eq!(get_mstv("gofmt"), Some(Version::new(1, 18, 0)));
  assert_eq!(get_mstv("golangci-lint"), Some(Version::new(1, 50, 0)));
}

#[test]
fn test_tool_mstv_registry_entries() {
  let yamllint_entry =
    get_tool_mstv_entry("yamllint").expect("yamllint registered");
  assert_eq!(yamllint_entry.min_version, Version::new(1, 20, 0));
  assert_eq!(yamllint_entry.version_args, &["--version"]);
  assert_eq!(yamllint_entry.regex, r"yamllint (\d+\.\d+\.\d+)");
  assert_eq!(
    yamllint_entry.advice,
    "Run 'pip install -U yamllint' or 'brew install yamllint'"
  );

  let typstyle_entry =
    get_tool_mstv_entry("typstyle").expect("typstyle registered");
  assert_eq!(typstyle_entry.min_version, Version::new(0, 11, 0));
  assert_eq!(typstyle_entry.version_args, &["--version"]);
  assert_eq!(typstyle_entry.regex, r"typstyle (\d+\.\d+\.\d+)");
  assert_eq!(
    typstyle_entry.advice,
    "Run 'cargo install --locked typstyle' or 'brew install typstyle'"
  );

  let biome_entry = get_tool_mstv_entry("biome").expect("biome registered");
  assert_eq!(biome_entry.min_version, Version::new(1, 5, 0));
  assert_eq!(biome_entry.version_args, &["--version"]);
  assert_eq!(biome_entry.regex, r"(\d+\.\d+\.\d+)");
  assert_eq!(
    biome_entry.advice,
    "Run 'npm install -g @biomejs/biome' or 'brew install biome'"
  );

  let checkstyle_entry =
    get_tool_mstv_entry("checkstyle").expect("checkstyle registered");
  assert_eq!(checkstyle_entry.min_version, Version::new(10, 0, 0));
  assert_eq!(checkstyle_entry.version_args, &["--version"]);
  assert_eq!(
    checkstyle_entry.regex,
    r"Checkstyle version:? (\d+\.\d+(?:\.\d+)?)"
  );
  assert_eq!(
    checkstyle_entry.advice,
    "Run 'brew install checkstyle' or update your checkstyle jar"
  );

  let ktfmt_entry = get_tool_mstv_entry("ktfmt").expect("ktfmt registered");
  assert_eq!(ktfmt_entry.min_version, Version::new(0, 44, 0));
  assert_eq!(ktfmt_entry.version_args, &["--version"]);
  assert_eq!(ktfmt_entry.regex, r"ktfmt version (\d+\.\d+(?:\.\d+)?)");
  assert_eq!(ktfmt_entry.advice, "Run 'brew install ktfmt'");

  let ktlint_entry = get_tool_mstv_entry("ktlint").expect("ktlint registered");
  assert_eq!(ktlint_entry.min_version, Version::new(1, 0, 0));
  assert_eq!(ktlint_entry.version_args, &["--version"]);
  assert_eq!(ktlint_entry.regex, r"(\d+\.\d+\.\d+)");
  assert_eq!(ktlint_entry.advice, "Run 'brew install ktlint'");

  let gofmt_entry = get_tool_mstv_entry("gofmt").expect("gofmt registered");
  assert_eq!(gofmt_entry.min_version, Version::new(1, 18, 0));
  assert_eq!(gofmt_entry.regex, r"go(\d+\.\d+(?:\.\d+)?)");
  assert_eq!(
    gofmt_entry.advice,
    "Update Go toolchain via https://go.dev/dl/"
  );

  let golangci_entry =
    get_tool_mstv_entry("golangci-lint").expect("golangci-lint registered");
  assert_eq!(golangci_entry.min_version, Version::new(1, 50, 0));
  assert_eq!(golangci_entry.version_args, &["version"]);
  assert_eq!(
    golangci_entry.regex,
    r"golangci-lint has version (\d+\.\d+\.\d+)"
  );
  assert_eq!(
    golangci_entry.advice,
    "Run 'brew install golangci-lint' or update via https://golangci-lint.run"
  );

  assert_eq!(
    tool_upgrade_advice("yamllint"),
    Some("Run 'pip install -U yamllint' or 'brew install yamllint'")
  );
  assert_eq!(
    tool_upgrade_advice("typstyle"),
    Some("Run 'cargo install --locked typstyle' or 'brew install typstyle'")
  );
  assert_eq!(
    tool_upgrade_advice("biome"),
    Some("Run 'npm install -g @biomejs/biome' or 'brew install biome'")
  );
  assert_eq!(
    tool_upgrade_advice("checkstyle"),
    Some("Run 'brew install checkstyle' or update your checkstyle jar")
  );
  assert_eq!(
    tool_upgrade_advice("ktfmt"),
    Some("Run 'brew install ktfmt'")
  );
  assert_eq!(
    tool_upgrade_advice("ktlint"),
    Some("Run 'brew install ktlint'")
  );
  assert_eq!(
    tool_upgrade_advice("gofmt"),
    Some("Update Go toolchain via https://go.dev/dl/")
  );
  assert_eq!(
    tool_upgrade_advice("golangci-lint"),
    Some(
      "Run 'brew install golangci-lint' or update via https://golangci-lint.run"
    )
  );

  assert!(all_mstv_entries().len() >= 16);
}

#[test]
fn test_compatibility_policy_evaluation() {
  let min = Version::new(1, 4, 0);

  let v_ok = Version::new(1, 7, 0);
  let status_ok = CompatibilityPolicy::evaluate(Some(&v_ok), &min);
  assert!(status_ok.is_compatible());
  assert!(!status_ok.is_outdated());
  assert!(!status_ok.is_not_found());
  assert!(!status_ok.is_unknown_version());
  assert_eq!(
    status_ok,
    ToolStatus::Compatible {
      current: v_ok.clone(),
      minimum: min.clone()
    }
  );
  assert_eq!(
    status_ok.to_string(),
    format!("Compatible ({} >= MSTV {})", v_ok, min)
  );

  let v_old = Version::new(1, 3, 9);
  let status_old = CompatibilityPolicy::evaluate(Some(&v_old), &min);
  assert!(!status_old.is_compatible());
  assert!(status_old.is_outdated());
  assert_eq!(
    status_old,
    ToolStatus::Outdated {
      current: v_old.clone(),
      minimum: min.clone()
    }
  );
  assert_eq!(
    status_old.to_string(),
    format!("Outdated ({} < MSTV {})", v_old, min)
  );

  let status_none = CompatibilityPolicy::evaluate(None, &min);
  assert!(status_none.is_not_found());
  assert_eq!(status_none.to_string(), "Not Found");

  let status_unknown = CompatibilityPolicy::evaluate_with_raw(
    None,
    Some("custom build vX.Y".to_string()),
    &min,
  );
  assert!(status_unknown.is_unknown_version());
  assert_eq!(
    status_unknown.to_string(),
    "Unknown Version (custom build vX.Y)"
  );
}

#[test]
fn test_from_str_trait() {
  let parsed: Result<Version, _> = "3.5.1".parse();
  assert_eq!(parsed, Ok(Version::new(3, 5, 1)));

  let bad: Result<Version, _> = "invalid-ver".parse();
  assert!(bad.is_err());
}

#[test]
fn test_check_tool_compatibility_missing_tool() {
  let status = check_tool_compatibility(
    "nonexistent_binary_xyz_123",
    &Version::new(1, 0, 0),
  );
  assert_eq!(status, ToolStatus::NotFound);
}

#[test]
fn test_live_probe_rustfmt() {
  if which::which("rustfmt").is_ok() {
    let ver = probe_tool_version("rustfmt");
    assert!(ver.is_some(), "Expected rustfmt version to be parsed");
    let mstv = minimum_supported_tool_version("rustfmt").unwrap();
    let status = check_tool_compatibility("rustfmt", &mstv);
    assert!(status.is_compatible(), "rustfmt should satisfy MSTV 1.4.0");
  }
}
