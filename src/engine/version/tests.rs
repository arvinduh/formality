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

  // A leading-zero *core* is malformed beyond salvage: `None` (surfaced as
  // UnknownVersion downstream), never a fabricated comparable version.
  assert_eq!(Version::parse("01.2.3"), None);
  assert_eq!(Version::parse("1.2.03"), None);

  // A present-but-non-numeric 3rd component cannot become a patch without
  // fabricating a lower-than-reality `X.Y.0`: reject, don't salvage.
  // (PEP440 separator-less prereleases like `0.9.6rc1` are real for
  // pip-installed ruff / yamllint.)
  assert_eq!(Version::parse("0.9.6rc1"), None);
  assert_eq!(Version::parse("1.35.dev1"), None);
  assert_eq!(Version::parse("1.2.x"), None);

  // A non-semver *suffix* after a valid `MAJOR.MINOR.PATCH` core (or a clean
  // 4th component) is salvaged down to that core — the pre-`semver` parser
  // ignored trailing junk too, and the patch is preserved, not zeroed.
  assert_eq!(Version::parse("1.0.0-01"), Some(Version::new(1, 0, 0)));
  assert_eq!(
    Version::parse("18.1.8-0ubuntu1~22.04.1"),
    Some(Version::new(18, 1, 8))
  );
  assert_eq!(Version::parse("1.35.1.post1"), Some(Version::new(1, 35, 1)));
  assert_eq!(Version::parse("0.9.6.dev0"), Some(Version::new(0, 9, 6)));
  // Dotted separator keeps the patch; only the 4th component is dropped.
  assert_eq!(Version::parse("0.9.6.rc1"), Some(Version::new(0, 9, 6)));

  // First-match-wins is bounded: a malformed-beyond-salvage version-shaped
  // token aborts the scan rather than skipping ahead to a later number.
  assert_eq!(Version::parse("weird 01.2.3 (built 2024.1.5)"), None);
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

  // #149: `-1ubuntu1` is a Debian/Ubuntu packaging *revision*, not a
  // prerelease -- it must salvage to the bare core, same verdict as the
  // `~`-mangled Ubuntu suffix below, not sort below `14.0.0` as a
  // prerelease would. This is a deliberate verdict change from #145, which
  // took the `semver`-valid-prerelease parse at face value here.
  let clang_tidy = "clang-tidy version 14.0.0-1ubuntu1";
  assert_eq!(Version::extract(clang_tidy), Some(Version::new(14, 0, 0)));

  // Ubuntu distro revision: `~` and a leading-zero identifier make the suffix
  // invalid semver, so it is salvaged to the bare core (Compatible, not
  // Unknown, against clang's MSTV of 14.0.0).
  let clang_fmt_ubuntu = "clang-format version 18.1.8-0ubuntu1~22.04.1";
  assert_eq!(
    Version::extract(clang_fmt_ubuntu),
    Some(Version::new(18, 1, 8))
  );
  let clang_tidy_ubuntu = "Ubuntu clang-tidy version 18.1.8-0ubuntu1~22.04.1";
  assert_eq!(
    Version::extract(clang_tidy_ubuntu),
    Some(Version::new(18, 1, 8))
  );

  // PyPI post/dev builds carry a non-numeric 4th component the pre-`semver`
  // parser ignored; the bare-core salvage keeps that behaviour.
  let yamllint_post = "yamllint 1.35.1.post1";
  assert_eq!(
    Version::extract(yamllint_post),
    Some(Version::new(1, 35, 1))
  );
  let ruff_dev = "ruff 0.9.6.dev0";
  assert_eq!(Version::extract(ruff_dev), Some(Version::new(0, 9, 6)));

  // ...but a PEP440 separator-less prerelease (`0.9.6rc1`) has no clean patch
  // to keep: it is rejected, not salvaged to a fabricated `0.9.0`.
  assert_eq!(Version::extract("ruff 0.9.6rc1"), None);
  assert_eq!(normalize_probed_version("ruff", "ruff 0.9.6rc1"), None);

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
fn test_distro_revision_vs_genuine_prerelease() {
  // #149: table-driven coverage of the extraction-layer heuristic that
  // distinguishes a packaging/distro revision suffix (sorts *with* its base
  // release, never below it) from a genuine prerelease (sorts below).
  //
  // (input, expected parse, is a prerelease?)
  let cases: &[(&str, Version, bool)] = &[
    // Plain releases: nothing to distinguish.
    ("1.2.3", Version::new(1, 2, 3), false),
    // Debian/Ubuntu packaging revisions: numeric-leading identifier with no
    // recognised prerelease keyword -> distro revision, bare core kept.
    ("1.2.3-1ubuntu2", Version::new(1, 2, 3), false),
    ("1.2.3-0ubuntu1", Version::new(1, 2, 3), false),
    // `~` makes the suffix invalid semver outright; already salvaged to the
    // bare core before the prerelease-keyword check even runs.
    ("1.2.3-0ubuntu1~22.04.1", Version::new(1, 2, 3), false),
    // RPM/Fedora revisions (`-<rev>.<dist-tag>`): first identifier is a bare
    // digit, same bucket as the Debian revisions above.
    ("1.2.3-4.fc39", Version::new(1, 2, 3), false),
    // Arch's single-integer package-revision suffix. Ambiguous in the
    // abstract (SemVer alone would read `-1` as a prerelease numeric
    // identifier), but Arch is the only convention that emits a bare
    // numeral here, and a real prerelease is never spelled as a plain
    // integer with no keyword -- tie-break: distro revision.
    ("1.2.3-1", Version::new(1, 2, 3), false),
    // Homebrew-style single-integer bottle/formula revision -- same bucket
    // and same tie-break as the Arch case.
    ("1.2.3-2", Version::new(1, 2, 3), false),
    // A distro-style tag with a non-numeric but non-keyword leading
    // identifier (a raw distro/codename prefix, not `alpha`/`beta`/etc.)
    // is still a revision, not a prerelease.
    ("1.2.3-ubuntu1", Version::new(1, 2, 3), false),
    // Genuine prereleases: recognised keyword leads the first identifier,
    // kept and must sort *below* the bare release.
    ("1.2.3-rc1", Version::with_prerelease(1, 2, 3, "rc1"), true),
    (
      "1.2.3-rc.1",
      Version::with_prerelease(1, 2, 3, "rc.1"),
      true,
    ),
    (
      "1.2.3-beta.2",
      Version::with_prerelease(1, 2, 3, "beta.2"),
      true,
    ),
    (
      "1.2.3-alpha",
      Version::with_prerelease(1, 2, 3, "alpha"),
      true,
    ),
    ("1.2.3-pre", Version::with_prerelease(1, 2, 3, "pre"), true),
    (
      "1.2.3-nightly",
      Version::with_prerelease(1, 2, 3, "nightly"),
      true,
    ),
    // Build metadata is dropped from ordering entirely (semver rule), not a
    // prerelease either way -- parses to the bare core with no prerelease.
    ("1.2.3+build.5", Version::new(1, 2, 3), false),
    // A genuine prerelease combined with build metadata: prerelease kept,
    // build metadata still dropped.
    (
      "1.2.3-rc1+build.5",
      Version::with_prerelease(1, 2, 3, "rc1"),
      true,
    ),
  ];

  for (input, expected, is_prerelease) in cases {
    let parsed = Version::parse(input);
    assert_eq!(parsed, Some(expected.clone()), "parsing {input:?}");
    assert_eq!(
      parsed.as_ref().unwrap().prerelease.is_some(),
      *is_prerelease,
      "prerelease-ness of {input:?}"
    );

    // MSTV outcome: a distro revision must compare >= its own bare release
    // (never Outdated against an MSTV equal to that release); a genuine
    // prerelease must compare < it (Outdated against that same floor).
    let base = Version::new(1, 2, 3);
    let status = evaluate_tool_status(parsed, None, Some(&base), None);
    if *is_prerelease {
      assert!(
        status.is_outdated(),
        "{input:?} (genuine prerelease) must be Outdated vs MSTV {base}, got {status:?}"
      );
    } else {
      assert!(
        status.is_compatible(),
        "{input:?} (distro revision or plain release) must be Compatible vs MSTV {base}, got {status:?}"
      );
    }
  }
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
  assert_eq!(v1_4_0, v1_4_0);

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

  assert!(all_mstv_entries().len() >= 16);
}

#[test]
fn test_evaluate_tool_status_basic_evaluation() {
  let min = Version::new(1, 4, 0);

  let v_ok = Version::new(1, 7, 0);
  let status_ok =
    evaluate_tool_status(Some(v_ok.clone()), None, Some(&min), None);
  assert!(status_ok.is_compatible());
  assert!(!status_ok.is_outdated());
  assert!(!status_ok.is_not_found());
  assert!(!status_ok.is_unknown_version());
  assert!(!status_ok.is_stale());
  assert_eq!(
    status_ok,
    ToolStatus::Compatible {
      current: v_ok.clone(),
      minimum: min.clone()
    }
  );
  assert_eq!(
    status_ok.to_string(),
    format!("Compatible ({v_ok} >= MSTV {min})")
  );

  let v_old = Version::new(1, 3, 9);
  let status_old =
    evaluate_tool_status(Some(v_old.clone()), None, Some(&min), None);
  assert!(!status_old.is_compatible());
  assert!(status_old.is_outdated());
  assert!(!status_old.is_not_found());
  assert!(!status_old.is_unknown_version());
  assert!(!status_old.is_stale());
  assert_eq!(
    status_old,
    ToolStatus::Outdated {
      current: v_old.clone(),
      minimum: min.clone()
    }
  );
  assert_eq!(
    status_old.to_string(),
    format!("Outdated ({v_old} < MSTV {min})")
  );

  let status_none = evaluate_tool_status(None, None, Some(&min), None);
  assert!(status_none.is_not_found());
  assert!(!status_none.is_compatible());
  assert!(!status_none.is_outdated());
  assert!(!status_none.is_unknown_version());
  assert!(!status_none.is_stale());
  assert_eq!(status_none.to_string(), "Not Found");

  let status_unknown = evaluate_tool_status(
    None,
    Some("custom build vX.Y".to_string()),
    Some(&min),
    None,
  );
  assert!(status_unknown.is_unknown_version());
  assert!(!status_unknown.is_compatible());
  assert!(!status_unknown.is_outdated());
  assert!(!status_unknown.is_not_found());
  assert!(!status_unknown.is_stale());
  assert_eq!(
    status_unknown.to_string(),
    "Unknown Version (custom build vX.Y)"
  );
}

#[test]
fn test_evaluate_tool_status_mstv_boundary_is_compatible() {
  // A tool at exactly the MSTV boundary (current == minimum) must be
  // Compatible, not Outdated — the `>=` comparison in `evaluate_tool_status`
  // must handle the equality edge properly.
  let min = Version::new(1, 4, 0);
  let exact = Version::new(1, 4, 0);

  let status =
    evaluate_tool_status(Some(exact.clone()), None, Some(&min), None);
  assert!(status.is_compatible());
  assert!(!status.is_outdated());
  assert_eq!(
    status,
    ToolStatus::Compatible {
      current: exact.clone(),
      minimum: min.clone()
    }
  );

  let status_raw = evaluate_tool_status(
    Some(exact),
    Some("rustfmt 1.4.0".to_string()),
    Some(&min),
    None,
  );
  assert!(status_raw.is_compatible());

  // One patch below the boundary must be Outdated.
  let just_below = Version::new(1, 3, 9);
  let status_below =
    evaluate_tool_status(Some(just_below), None, Some(&min), None);
  assert!(status_below.is_outdated());

  // A prerelease *at* the boundary triple stays below it (semver precedence
  // rule 9), matching the pre-semver verdict — the floor check is delegated
  // to `semver` ordering, not `VersionReq`.
  let at_boundary_pre = Version::with_prerelease(1, 4, 0, "rc.1");
  let status_pre =
    evaluate_tool_status(Some(at_boundary_pre), None, Some(&min), None);
  assert!(status_pre.is_outdated());
}

#[test]
fn test_salvaged_distro_build_still_gets_a_real_mstv_verdict() {
  // End to end: an Ubuntu clang-format banner whose distro-revision suffix is
  // not valid semver must still yield a comparable version and a real MSTV
  // verdict -- not `UnknownVersion`, which would silently drop the check on a
  // normal Ubuntu dev box (QA finding #1).
  let raw = "clang-format version 18.1.8-0ubuntu1~22.04.1";
  let current = normalize_probed_version("clang-format", raw);
  assert_eq!(current, Some(Version::new(18, 1, 8)));

  let min = Version::new(14, 0, 0);
  let status =
    evaluate_tool_status(current, Some(raw.to_string()), Some(&min), None);
  assert!(status.is_compatible());
  assert!(!status.is_unknown_version());
}

#[test]
fn test_evaluate_tool_status_raw_and_none_paths() {
  let min = Version::new(1, 0, 0);

  // Neither a parsed version nor raw output at all: NotFound.
  let status_neither = evaluate_tool_status(None, None, Some(&min), None);
  assert!(status_neither.is_not_found());

  // Raw output present but empty/whitespace-only: still NotFound, not
  // UnknownVersion — an empty banner carries no diagnostic value.
  let status_blank =
    evaluate_tool_status(None, Some("   ".to_string()), Some(&min), None);
  assert!(status_blank.is_not_found());

  // A parsed current version takes precedence over raw output entirely,
  // even when both are present.
  let status_both = evaluate_tool_status(
    Some(Version::new(2, 0, 0)),
    Some("garbage banner text".to_string()),
    Some(&min),
    None,
  );
  assert!(status_both.is_compatible());
}

#[test]
fn test_get_tool_mstv_entry_clippy_aliases_resolve_to_same_entry() {
  // clippy-driver / cargo-clippy are alternate binary names for the same
  // logical "clippy" tool; get_tool_mstv_entry must alias them to the
  // single `clippy` registry entry rather than treating them as unknown.
  let canonical = get_tool_mstv_entry("clippy").expect("clippy registered");
  let via_driver =
    get_tool_mstv_entry("clippy-driver").expect("clippy-driver aliases");
  let via_cargo =
    get_tool_mstv_entry("cargo-clippy").expect("cargo-clippy aliases");

  assert_eq!(canonical.binary, "clippy");
  assert_eq!(via_driver.binary, "clippy");
  assert_eq!(via_cargo.binary, "clippy");
  assert_eq!(canonical.min_version, via_driver.min_version);
  assert_eq!(canonical.min_version, via_cargo.min_version);
}

#[test]
fn test_from_str_trait() {
  let parsed: Result<Version, _> = "3.5.1".parse();
  assert_eq!(parsed, Ok(Version::new(3, 5, 1)));

  let bad: Result<Version, _> = "invalid-ver".parse();
  assert!(bad.is_err());
}

#[test]
fn test_evaluate_tool_status_pin_match_is_compatible() {
  // Present, above MSTV, and matches the pin exactly: READY, not STALE.
  let current = Version::new(3, 9, 6);
  let minimum = Version::new(2, 0, 0);
  let pinned = Version::new(3, 9, 6);
  let status = evaluate_tool_status(
    Some(current.clone()),
    Some("prettier 3.9.6".to_string()),
    Some(&minimum),
    Some(&pinned),
  );
  assert!(status.is_compatible());
  assert!(!status.is_stale());
  assert_eq!(
    status,
    ToolStatus::Compatible {
      current,
      minimum: minimum.clone(),
    }
  );
}

#[test]
fn test_evaluate_tool_status_pin_mismatch_is_stale() {
  // The #5 repro: a stale system-wide prettier 3.8.1 with pin 3.9.6 -- above
  // MSTV (so it "works"), but not the exact bits `fml install` would pin.
  let current = Version::new(3, 8, 1);
  let minimum = Version::new(2, 0, 0);
  let pinned = Version::new(3, 9, 6);
  let status = evaluate_tool_status(
    Some(current.clone()),
    Some("prettier 3.8.1".to_string()),
    Some(&minimum),
    Some(&pinned),
  );
  assert!(status.is_stale());
  assert!(!status.is_compatible());
  assert!(!status.is_outdated());
  assert_eq!(
    status,
    ToolStatus::Stale {
      current: current.clone(),
      pinned: pinned.clone(),
    }
  );
  assert_eq!(
    status.to_string(),
    format!("Stale ({current} != pinned {pinned})")
  );
}

#[test]
fn test_evaluate_tool_status_below_mstv_outdated_beats_pin_mismatch() {
  // A tool that is BOTH below the MSTV floor AND mismatched against the pin
  // must report Outdated, not Stale: "might not even work" outranks "works,
  // just isn't the exact pin".
  let current = Version::new(1, 0, 0);
  let minimum = Version::new(2, 0, 0);
  let pinned = Version::new(3, 9, 6);
  let status = evaluate_tool_status(
    Some(current.clone()),
    Some("tool 1.0.0".to_string()),
    Some(&minimum),
    Some(&pinned),
  );
  assert!(status.is_outdated());
  assert!(!status.is_stale());
}

#[test]
fn test_evaluate_tool_status_absent_tool_is_not_found_regardless_of_pin() {
  // Tool absent entirely: NotFound, unaffected by whether a pin/minimum is
  // configured -- MISS stays MISS, it never becomes STALE.
  let minimum = Version::new(2, 0, 0);
  let pinned = Version::new(3, 9, 6);
  let status = evaluate_tool_status(None, None, Some(&minimum), Some(&pinned));
  assert!(status.is_not_found());
}

#[test]
fn test_evaluate_tool_status_no_pinned_version_configured_never_stale() {
  // A tool with an MSTV floor but no known pin (e.g. no install chain, or no
  // installer currently available to resolve one from) must never report
  // Stale -- there is nothing to compare against, so it falls back to the
  // existing MSTV-only Compatible/Outdated behavior.
  let current = Version::new(5, 0, 0);
  let minimum = Version::new(1, 4, 0);
  let status = evaluate_tool_status(
    Some(current.clone()),
    Some("tool 5.0.0".to_string()),
    Some(&minimum),
    None,
  );
  assert!(status.is_compatible());
  assert!(!status.is_stale());
}

#[test]
fn test_evaluate_tool_status_unparsed_version_fails_soft_to_unknown() {
  // A tool whose `--version` banner doesn't parse into a semver: doctor must
  // not crash, and must not silently claim Stale/Compatible about a version
  // it never actually understood -- UnknownVersion, carrying the raw banner.
  let pinned = Version::new(3, 9, 6);
  let status = evaluate_tool_status(
    None,
    Some("custom build, no version number".to_string()),
    None,
    Some(&pinned),
  );
  assert!(status.is_unknown_version());

  // The regression this refactor must not introduce: even with an MSTV floor
  // present, an unparseable version is UnknownVersion -- never Compatible,
  // never Outdated, never silently "satisfies the minimum".
  let floored = evaluate_tool_status(
    None,
    Some("custom build, no version number".to_string()),
    Some(&Version::new(1, 4, 0)),
    Some(&pinned),
  );
  assert!(floored.is_unknown_version());
  assert!(!floored.is_compatible() && !floored.is_outdated());
}

#[test]
fn test_tool_status_stale_display_and_predicate() {
  let status = ToolStatus::Stale {
    current: Version::new(3, 8, 1),
    pinned: Version::new(3, 9, 6),
  };
  assert!(status.is_stale());
  assert!(!status.is_compatible());
  assert!(!status.is_outdated());
  assert!(!status.is_not_found());
  assert!(!status.is_unknown_version());
  assert_eq!(status.to_string(), "Stale (3.8.1 != pinned 3.9.6)");
}

#[test]
fn test_live_probe_rustfmt() {
  if which::which("rustfmt").is_ok() {
    let ver = probe_tool_version("rustfmt");
    assert!(ver.is_some(), "Expected rustfmt version to be parsed");
    let mstv = minimum_supported_tool_version("rustfmt").unwrap();
    let raw = get_raw_tool_version("rustfmt");
    let status = evaluate_tool_status(ver, raw, Some(&mstv), None);
    assert!(status.is_compatible(), "rustfmt should satisfy MSTV 1.4.0");
  }
}

#[test]
fn test_tool_status_unknown_version_display_and_predicates() {
  let status_raw = ToolStatus::UnknownVersion("nightly-2026".to_string());
  assert!(status_raw.is_unknown_version());
  assert!(!status_raw.is_compatible());
  assert!(!status_raw.is_not_found());
  assert_eq!(status_raw.to_string(), "Unknown Version (nightly-2026)");

  let status_empty = ToolStatus::UnknownVersion(String::new());
  assert!(status_empty.is_unknown_version());
  assert!(!status_empty.is_compatible());
  assert!(!status_empty.is_not_found());
  assert_eq!(status_empty.to_string(), "Unknown Version (probe failed)");
}

#[test]
fn test_normalize_probed_version() {
  assert_eq!(
    normalize_probed_version("rustfmt", "rustfmt 1.7.0 (7576e26b 2024-05-07)"),
    Some(Version::new(1, 7, 0))
  );
  assert_eq!(
    normalize_probed_version("ruff", "ruff 0.9.6"),
    Some(Version::new(0, 9, 6))
  );

  // Clippy 0.1.X is remapped to 1.X.0 for clippy, clippy-driver, and cargo-clippy
  assert_eq!(
    normalize_probed_version("clippy", "clippy 0.1.65 (rustc 1.65.0)"),
    Some(Version::new(1, 65, 0))
  );
  assert_eq!(
    normalize_probed_version("clippy-driver", "clippy 0.1.70"),
    Some(Version::new(1, 70, 0))
  );
  assert_eq!(
    normalize_probed_version(
      "cargo-clippy",
      "clippy 0.1.80-nightly (rustc 1.80.0)"
    ),
    Some(Version::with_prerelease(1, 80, 0, "nightly"))
  );

  // Non-clippy tools with 0.1.X are NOT remapped
  assert_eq!(
    normalize_probed_version("other-tool", "other-tool 0.1.65"),
    Some(Version::new(0, 1, 65))
  );

  // Invalid strings return None
  assert_eq!(normalize_probed_version("rustfmt", ""), None);
  assert_eq!(normalize_probed_version("clippy", "invalid banner"), None);
}

#[test]
fn test_tool_version_store_serialization_roundtrip() {
  let temp = tempfile::TempDir::new().unwrap();
  let cache_path = temp.path().join("tool_versions.json");

  let mut store = ToolVersionStore::default();
  store.tools.insert(
    "rustfmt".to_string(),
    ToolVersionEntry {
      raw_version: "rustfmt 1.7.0".to_string(),
      last_checked_unix: 1700000000,
      binary_mtime_unix: 1699999000,
      binary_path: Some("/bin/rustfmt".to_string()),
    },
  );
  store.tools.insert(
    "ruff".to_string(),
    ToolVersionEntry {
      raw_version: "ruff 0.9.6".to_string(),
      last_checked_unix: 1700000100,
      binary_mtime_unix: 1699999100,
      binary_path: None,
    },
  );

  write_tool_version_cache_at(&cache_path, &store);
  let loaded = read_tool_version_cache_at(&cache_path).expect("valid cache");
  assert_eq!(loaded, store);
  assert_eq!(loaded.tools.len(), 2);
  assert_eq!(
    loaded.tools.get("rustfmt").unwrap().raw_version,
    "rustfmt 1.7.0"
  );
  assert_eq!(loaded.tools.get("ruff").unwrap().raw_version, "ruff 0.9.6");
}

#[test]
fn test_tool_version_cache_miss_populates_cache() {
  let temp = tempfile::TempDir::new().unwrap();
  let cache_path = temp.path().join("tool_versions.json");

  assert!(read_tool_version_cache_at(&cache_path).is_none());

  if which::which("rustfmt").is_ok() {
    let raw = get_raw_tool_version_at("rustfmt", &cache_path);
    assert!(raw.is_some(), "Expected rustfmt raw output");

    let cache = read_tool_version_cache_at(&cache_path)
      .expect("cache file should be created on miss");
    let entry = cache
      .tools
      .get("rustfmt")
      .expect("rustfmt entry should be cached");
    assert_eq!(Some(&entry.raw_version), raw.as_ref());
    assert!(entry.last_checked_unix > 0);
  }
}

#[test]
fn test_tool_version_cache_hit_avoids_subprocess() {
  let temp = tempfile::TempDir::new().unwrap();
  let cache_path = temp.path().join("tool_versions.json");

  if which::which("rustfmt").is_ok() {
    let (bin_path, bin_mtime) =
      resolve_binary_info("rustfmt").expect("rustfmt binary info");

    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();

    let mut store = ToolVersionStore::default();
    store.tools.insert(
      "rustfmt".to_string(),
      ToolVersionEntry {
        raw_version: "rustfmt 99.88.77 (mocked cache hit)".to_string(),
        last_checked_unix: now,
        binary_mtime_unix: bin_mtime,
        binary_path: Some(bin_path.to_string_lossy().to_string()),
      },
    );
    write_tool_version_cache_at(&cache_path, &store);

    let raw = get_raw_tool_version_at("rustfmt", &cache_path);
    assert_eq!(raw, Some("rustfmt 99.88.77 (mocked cache hit)".to_string()));

    let probed = probe_tool_version_at("rustfmt", &cache_path);
    assert_eq!(probed, Some(Version::new(99, 88, 77)));
  }
}

#[test]
fn test_tool_version_cache_mtime_invalidation() {
  let temp = tempfile::TempDir::new().unwrap();
  let cache_path = temp.path().join("tool_versions.json");

  if which::which("rustfmt").is_ok() {
    let (bin_path, bin_mtime) =
      resolve_binary_info("rustfmt").expect("rustfmt binary info");

    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();

    // Cache has mismatched mtime (e.g. tool binary was upgraded on disk)
    let stale_mtime = bin_mtime.wrapping_sub(500);
    let mut store = ToolVersionStore::default();
    store.tools.insert(
      "rustfmt".to_string(),
      ToolVersionEntry {
        raw_version: "rustfmt 99.88.77 (stale mtime)".to_string(),
        last_checked_unix: now,
        binary_mtime_unix: stale_mtime,
        binary_path: Some(bin_path.to_string_lossy().to_string()),
      },
    );
    write_tool_version_cache_at(&cache_path, &store);

    let raw = get_raw_tool_version_at("rustfmt", &cache_path);
    assert_ne!(
      raw,
      Some("rustfmt 99.88.77 (stale mtime)".to_string()),
      "mismatched mtime should invalidate cached version"
    );

    let updated_cache = read_tool_version_cache_at(&cache_path)
      .expect("cache file should be updated");
    let entry = updated_cache.tools.get("rustfmt").unwrap();
    assert_eq!(entry.binary_mtime_unix, bin_mtime);
  }
}

#[test]
fn test_tool_version_cache_ttl_invalidation() {
  let temp = tempfile::TempDir::new().unwrap();
  let cache_path = temp.path().join("tool_versions.json");

  if which::which("rustfmt").is_ok() {
    let (bin_path, bin_mtime) =
      resolve_binary_info("rustfmt").expect("rustfmt binary info");

    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();

    // Cache has expired timestamp (older than TTL)
    let expired_time = now - (TOOL_VERSION_CACHE_TTL_SECS + 120);
    let mut store = ToolVersionStore::default();
    store.tools.insert(
      "rustfmt".to_string(),
      ToolVersionEntry {
        raw_version: "rustfmt 99.88.77 (expired TTL)".to_string(),
        last_checked_unix: expired_time,
        binary_mtime_unix: bin_mtime,
        binary_path: Some(bin_path.to_string_lossy().to_string()),
      },
    );
    write_tool_version_cache_at(&cache_path, &store);

    let raw = get_raw_tool_version_at("rustfmt", &cache_path);
    assert_ne!(
      raw,
      Some("rustfmt 99.88.77 (expired TTL)".to_string()),
      "expired TTL should invalidate cached version"
    );

    let updated_cache = read_tool_version_cache_at(&cache_path)
      .expect("cache file should be updated");
    let entry = updated_cache.tools.get("rustfmt").unwrap();
    assert!(entry.last_checked_unix >= now);
  }
}

#[test]
fn test_tool_version_cache_corrupted_json_resilience() {
  let temp = tempfile::TempDir::new().unwrap();
  let cache_path = temp.path().join("tool_versions.json");

  std::fs::write(&cache_path, "not valid json {{{").unwrap();
  assert_eq!(read_tool_version_cache_at(&cache_path), None);

  if which::which("rustfmt").is_ok() {
    let raw = get_raw_tool_version_at("rustfmt", &cache_path);
    assert!(raw.is_some());

    let updated = read_tool_version_cache_at(&cache_path)
      .expect("corrupted cache should be cleanly overwritten with valid JSON");
    assert!(updated.tools.contains_key("rustfmt"));
  }
}
