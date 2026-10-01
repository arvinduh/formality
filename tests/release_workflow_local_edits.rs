//! Guards the hand-applied local edits carried by the otherwise
//! dist-generated `.github/workflows/release.yml`.
//!
//! `Cargo.toml` sets `allow-dirty = ["ci"]`, which is what lets those edits
//! survive in the tree at all — but it also makes
//! `dist generate --mode=ci --check` refuse to run rather than report drift,
//! so dist itself can no longer tell anyone the edits are gone. Re-running
//! `dist generate --mode=ci --allow-dirty` after a cargo-dist bump silently
//! reverts every one of them, and the most dangerous revert
//! (`--generate-notes` back to dist's `--notes-file` changelog body) fails
//! *silently at release time*: this repo has no committed `CHANGELOG.md`, so
//! the published release body would simply be empty and nobody would notice
//! until a user looked at a release page.
//!
//! This test is the tooling behind the `# LOCAL EDIT (issue #N)` comments
//! in that workflow: a comment explains the edit, this asserts it is still
//! there. It runs in the `Library Tests` job (`cargo test --verbose`), one of
//! the repo's required status checks, so losing an edit blocks the merge that
//! lost it instead of surfacing at the next release.
//!
//! **If a future cargo-dist version makes one of these edits unnecessary,
//! deleting the edit is one commit: drop its `LocalEdit` entry below and its
//! `# LOCAL EDIT` comment from the workflow.** Nothing else references them.
//!
//! Additionally, this test suite guards the tag filter glob alignment between
//! `.github/workflows/release.yml` and `.github/workflows/release-extras.yml`
//! (issue #165).

use std::fs;
use std::path::PathBuf;

/// The comment marker each local edit carries in the workflow, up to the
/// issue number that introduced it. The count of these is asserted to equal
/// `EDITS.len()`, so an edit and its explanatory comment can never drift
/// apart.
const MARKER: &str = "# LOCAL EDIT (issue #";

/// One hand-applied edit to the dist-generated release workflow.
struct LocalEdit {
  /// Short name for the edit, used in the failure message.
  name: &'static str,
  /// Substrings that must still appear on a non-comment line of the
  /// workflow. Comment lines are excluded deliberately: the `LOCAL EDIT`
  /// comments quote what they replaced, and a quoted string in prose must
  /// not be able to satisfy (or trip) an assertion about live YAML.
  required: &'static [&'static str],
  /// Substrings dist's own generated output would reintroduce if the edit
  /// were reverted. Checked on non-comment lines only, for the same reason.
  forbidden: &'static [&'static str],
  /// What silently breaks if this edit is lost — quoted verbatim in the
  /// failure message so the reader does not have to go dig for the stakes.
  consequence: &'static str,
  /// Where in the parsed workflow the edit must sit to take effect; a
  /// needle found anywhere else does not count.
  site: Site,
}

/// The place in the workflow's YAML structure that an edit only works in.
enum Site {
  /// The top-level `on.push.tags` trigger lists exactly this glob.
  PushTag(&'static str),
  /// A single step of `job` holds every `required` needle, and runs after
  /// the step whose `id` or `name` is `after` and before the one that is
  /// `before`.
  Step {
    job: &'static str,
    after: Option<&'static str>,
    before: Option<&'static str>,
  },
}

const EDITS: &[LocalEdit] = &[
  LocalEdit {
    name: "tag glob constrained to a leading `v`",
    required: &["- 'v[0-9]+.[0-9]+.[0-9]+*'"],
    forbidden: &["- '**[0-9]+.[0-9]+.[0-9]+*'"],
    consequence: "dist's default prefix-less glob matches any tag ending in a version, \
       so a non-`v` tag would kick off a binary release that \
       release-extras.yml (`v*` only) never adds its assets to.",
    site: Site::PushTag("v[0-9]+.[0-9]+.[0-9]+*"),
  },
  LocalEdit {
    name: "`fetch-depth: 0` on the host job's checkout",
    required: &["fetch-depth: 0"],
    forbidden: &[],
    consequence: "the shallow default checkout has no tag history, so the release step \
       cannot resolve the previous `v*` tag for --notes-start-tag.",
    site: Site::Step {
      job: "host",
      after: None,
      before: Some("Create GitHub Release"),
    },
  },
  LocalEdit {
    name: "`gh release create --generate-notes --notes-start-tag`",
    required: &["--generate-notes", "--notes-start-tag"],
    forbidden: &["--notes-file"],
    consequence: "reverting to dist's --notes-file changelog body FAILS SILENTLY — this \
       repo has no committed CHANGELOG.md, so releases would publish with an \
       empty body and nothing would fail loudly.",
    site: Site::Step {
      job: "host",
      after: Some("Download GitHub Artifacts"),
      before: None,
    },
  },
  LocalEdit {
    name: "ARM64 Windows x64-fallback note patched into fml-installer.ps1",
    required: &[
      "installer=target/distrib/fml-installer.ps1",
      "runs under Windows' built-in x64 emulation. This is deliberate",
    ],
    forbidden: &[],
    consequence: "dist's PowerShell installer silently installs the x64 build on \
       ARM64 Windows; without this note users get an emulated binary they \
       never chose and are never told about (issue #166).",
    site: Site::Step {
      job: "build-global-artifacts",
      after: Some("cargo-dist"),
      before: Some("Upload artifacts"),
    },
  },
];

/// Returns the workflow's lines with whole-line YAML comments removed, so
/// assertions only ever see live configuration.
fn non_comment_lines(workflow: &str) -> String {
  workflow
    .lines()
    .filter(|line| !line.trim_start().starts_with('#'))
    .collect::<Vec<_>>()
    .join("\n")
}

fn read_workflow() -> String {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join(".github")
    .join("workflows")
    .join("release.yml");
  assert!(
    path.exists(),
    ".github/workflows/release.yml not found. It is generated by \
     `dist generate --mode=ci --allow-dirty` and carries hand-applied local \
     edits; if the release pipeline has moved, update this test's path (and \
     the edit table in it) to match."
  );
  fs::read_to_string(&path)
    .expect("Failed to read .github/workflows/release.yml")
}

fn read_release_extras_workflow() -> String {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join(".github")
    .join("workflows")
    .join("release-extras.yml");
  assert!(
    path.exists(),
    ".github/workflows/release-extras.yml not found; if the release pipeline \
     has moved, update this test's path accordingly."
  );
  fs::read_to_string(&path)
    .expect("Failed to read .github/workflows/release-extras.yml")
}

/// Flattens a step into searchable text: every scalar leaf as both
/// `key: value` and bare `value`, so a needle like `fetch-depth: 0` or a
/// fragment of a `run:` script matches the parsed step, not raw lines.
fn step_texts(value: &serde_yaml::Value, key: &str, out: &mut Vec<String>) {
  let scalar = match value {
    serde_yaml::Value::String(s) => Some(s.clone()),
    serde_yaml::Value::Number(n) => Some(n.to_string()),
    serde_yaml::Value::Bool(b) => Some(b.to_string()),
    _ => None,
  };
  if let Some(scalar) = scalar {
    out.push(format!("{key}: {scalar}"));
    out.push(scalar);
  } else if let Some(map) = value.as_mapping() {
    for (k, v) in map {
      step_texts(v, k.as_str().unwrap_or_default(), out);
    }
  } else if let Some(seq) = value.as_sequence() {
    for v in seq {
      step_texts(v, key, out);
    }
  }
}

/// Finds the index of the step whose `id` or `name` is `label`.
fn step_index(steps: &[serde_yaml::Value], label: &str) -> Option<usize> {
  steps.iter().position(|step| {
    ["id", "name"]
      .iter()
      .any(|field| step.get(field).and_then(|v| v.as_str()) == Some(label))
  })
}

/// Explains how `edit` misses its `site`, or returns `None` when it sits
/// there. `push_tags` is the workflow's parsed `on.push.tags` list.
fn misplacement(
  workflow: &serde_yaml::Value,
  push_tags: &[String],
  edit: &LocalEdit,
) -> Option<String> {
  let (job, after, before) = match edit.site {
    Site::PushTag(glob) => {
      return (!push_tags.iter().any(|tag| tag == glob)).then(|| {
        format!("`on.push.tags` does not list `{glob}` (found {push_tags:?})")
      });
    }
    Site::Step { job, after, before } => (job, after, before),
  };
  let Some(steps) = workflow
    .get("jobs")
    .and_then(|jobs| jobs.get(job))
    .and_then(|j| j.get("steps"))
    .and_then(|s| s.as_sequence())
  else {
    return Some(format!("job `{job}` has no `steps`"));
  };
  let Some(at) = steps.iter().position(|step| {
    let mut texts = Vec::new();
    step_texts(step, "", &mut texts);
    edit
      .required
      .iter()
      .all(|needle| texts.iter().any(|t| t.contains(needle)))
  }) else {
    return Some(format!(
      "no single step of job `{job}` holds all of {:?}",
      edit.required
    ));
  };
  for (label, side) in [(after, "after"), (before, "before")] {
    let Some(label) = label else { continue };
    let Some(other) = step_index(steps, label) else {
      return Some(format!("job `{job}` has no step `{label}`"));
    };
    let in_order = if side == "after" {
      at > other
    } else {
      at < other
    };
    if !in_order {
      return Some(format!(
        "its step in job `{job}` is not {side} step `{label}`"
      ));
    }
  }
  None
}

/// Extracts the list of tag filter globs under `on.push.tags` from a workflow YAML string.
fn extract_push_tags(workflow_yaml: &str, file_name: &str) -> Vec<String> {
  let val: serde_yaml::Value = serde_yaml::from_str(workflow_yaml)
    .unwrap_or_else(|e| panic!("Failed to parse {file_name} as YAML: {e}"));
  let on = val
    .get("on")
    .or_else(|| val.get(serde_yaml::Value::Bool(true)))
    .unwrap_or_else(|| panic!("{file_name} missing top-level `on` trigger"));
  let push = on
    .get("push")
    .unwrap_or_else(|| panic!("{file_name} missing `push` trigger under `on`"));
  let tags = push.get("tags").unwrap_or_else(|| {
    panic!("{file_name} missing `tags` filter under `push`")
  });
  if let Some(seq) = tags.as_sequence() {
    seq
      .iter()
      .map(|item| {
        item
          .as_str()
          .unwrap_or_else(|| {
            panic!("{file_name} tag filter element is not a string: {item:?}")
          })
          .to_string()
      })
      .collect()
  } else if let Some(s) = tags.as_str() {
    vec![s.to_string()]
  } else {
    panic!(
      "{file_name} `tags` filter under `push` is neither a sequence nor a string: {tags:?}"
    );
  }
}

#[test]
fn test_release_yml_local_edits_survive() {
  let workflow = read_workflow();
  let live = non_comment_lines(&workflow);

  let mut failures: Vec<String> = Vec::new();

  for edit in EDITS {
    for needle in edit.required {
      if !live.contains(needle) {
        failures.push(format!(
          "LOST: {}\n    expected `{}` in .github/workflows/release.yml, not found\n    why it matters: {}",
          edit.name, needle, edit.consequence
        ));
      }
    }
    for needle in edit.forbidden {
      if live.contains(needle) {
        failures.push(format!(
          "REVERTED: {}\n    `{}` is back in .github/workflows/release.yml — that is dist's generated form, not this repo's\n    why it matters: {}",
          edit.name, needle, edit.consequence
        ));
      }
    }
  }

  assert!(
    failures.is_empty(),
    "\n\n.github/workflows/release.yml has lost {} of its hand-applied local edits:\n\n  {}\n\n\
     This file is generated by cargo-dist; `dist generate --mode=ci --allow-dirty` reverts these edits \
     without warning, and `allow-dirty = [\"ci\"]` in Cargo.toml means `dist generate --check` cannot report it. \
     To re-apply: each edit is described in a `{}` comment in the workflow explaining exactly what to change. \
     If those comments are gone too, recover them with `git log -p -- .github/workflows/release.yml` \
     (see the issue each marker names), and see docs/release.md for the release procedure.\n\
     If a cargo-dist upgrade genuinely made an edit unnecessary, delete its entry from EDITS in \
     tests/release_workflow_local_edits.rs in the same commit that drops the edit.\n",
    failures.len(),
    failures.join("\n\n  "),
    MARKER
  );
}

/// Asserts each local edit sits in the job, and the step order, it only
/// works in: a re-application after `dist generate` that lands an edit on
/// the wrong job, or below the upload of the file it patches, keeps every
/// substring present but ships dist's behaviour (issue #412).
#[test]
fn test_release_yml_local_edits_are_in_place() {
  let raw = read_workflow();
  let workflow: serde_yaml::Value = serde_yaml::from_str(&raw)
    .expect("Failed to parse .github/workflows/release.yml as YAML");
  let push_tags = extract_push_tags(&raw, ".github/workflows/release.yml");

  let failures: Vec<String> = EDITS
    .iter()
    .filter_map(|edit| {
      misplacement(&workflow, &push_tags, edit).map(|why| {
        format!(
          "MISPLACED: {}\n    {why}\n    why it matters: {}",
          edit.name, edit.consequence
        )
      })
    })
    .collect();

  assert!(
    failures.is_empty(),
    "\n\n.github/workflows/release.yml has {} local edit(s) present but out of \
     place:\n\n  {}\n\nMove each back to the job and step order its `{}` \
     comment describes.\n",
    failures.len(),
    failures.join("\n\n  "),
    MARKER
  );
}

#[test]
fn test_release_yml_local_edits_are_each_documented() {
  let workflow = read_workflow();
  let found = workflow.matches(MARKER).count();

  assert_eq!(
    found,
    EDITS.len(),
    "\n\n.github/workflows/release.yml has {} `{}` comment(s), but {} local edit(s) are asserted in \
     tests/release_workflow_local_edits.rs. Every local edit must carry the marker comment explaining \
     why it exists and how to re-apply it after `dist generate` — that comment is the re-apply \
     instructions the assertion failure points readers at. Add the missing comment, or, if an edit was \
     deliberately dropped, remove its entry from EDITS in the same commit.\n",
    found,
    MARKER,
    EDITS.len()
  );
}

/// Asserts that `.github/workflows/release.yml` and
/// `.github/workflows/release-extras.yml` define identical tag filter globs.
///
/// Both workflows are triggered by release tags, with `release.yml` creating
/// the release and `release-extras.yml` polling for it to attach additional
/// assets (VS Code extension and JSON schema). If their tag filters diverge,
/// tags matching `release-extras.yml` but not `release.yml` would cause
/// `release-extras.yml` to burn its full 30-minute runner budget waiting for
/// a release that will never exist (see issue #165).
#[test]
fn test_release_extras_and_release_yml_tag_filters_match() {
  let release_yml = read_workflow();
  let release_extras_yml = read_release_extras_workflow();

  let release_tags =
    extract_push_tags(&release_yml, ".github/workflows/release.yml");
  let release_extras_tags = extract_push_tags(
    &release_extras_yml,
    ".github/workflows/release-extras.yml",
  );

  assert!(
    !release_tags.is_empty(),
    ".github/workflows/release.yml has no tag filters under push.tags"
  );

  assert_eq!(
    release_tags, release_extras_tags,
    "\n\nTag filter drift detected between release workflows!\n\
     .github/workflows/release.yml has: {:?}\n\
     .github/workflows/release-extras.yml has: {:?}\n\n\
     These must match exactly so that release-extras.yml does not trigger on tags \
     that cargo-dist ignores (which would burn a 30-minute runner timeout polling for \
     a release that is never created; see issue #165).\n",
    release_tags, release_extras_tags
  );
}
