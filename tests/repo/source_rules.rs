//! Source-tree rules that rustc and clippy cannot express.
//!
//! Each test walks `src/` (and some `tests/`) as text and enforces one rule
//! from `docs/style-guide.md`: file layout, doc comments, import shape, issue
//! citations, and what unit tests may call. Behavioural tests live with the
//! code they test.

use std::env;
use std::path;

// Tier-2 enforcement for docs/style-guide.md §1: unit tests live inline as
// `#[cfg(test)] mod tests { ... }` in the file under test, and integration
// tests live under `tests/`. No test file of any name lives in `src/`.
#[test]
fn src_has_no_test_files() {
  let src_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
  let violations: Vec<String> = ignore::WalkBuilder::new(&src_dir)
    .standard_filters(false)
    .build()
    .filter_map(Result::ok)
    .map(ignore::DirEntry::into_path)
    .filter(|p| {
      p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "tests.rs" || n.ends_with("_tests.rs"))
    })
    .map(|p| p.display().to_string())
    .collect();
  assert!(
    violations.is_empty(),
    "test files in src/ — inline them as `#[cfg(test)] mod tests`, or move \
     integration tests under tests/ (docs/style-guide.md §1):\n{}",
    violations.join("\n")
  );
}

// Tier-2 enforcement for the naming-conventions predicate-method rule
// documented in docs/style-guide.md §2 ("a pure getter or predicate ...
// carries #[must_use]"), promoted from tier 3 during #133 [pre-recreation]'s sweep.
//
// Normalizes the signature before matching: strips a leading `pub` /
// `pub(crate)` / `pub(super)` / `pub(in ...)` visibility modifier and any
// `const`/`async`/`unsafe` qualifiers (in any order), then requires the
// remainder to start with `fn is_`. Signatures are joined across lines up
// to the opening `{` (or a trailing `;` for a trait-method declaration)
// before checking for `-> bool`, so a return type on its own line is not
// invisible to the scan. This was proven against a real gap in an earlier
// version of this test: it matched only single-line `pub fn is_...(...)
// -> bool` signatures, which passed green even with `#[must_use]` deleted
// from `ExitStatus::is_clean` (a `pub const fn`) — i.e. it didn't catch
// the rule's own named exemplar. See docs/style-guide.md §2.
#[test]
fn is_predicate_methods_carry_must_use() {
  // Strips a leading `pub`/`pub(...)` visibility modifier and any
  // `const`/`async`/`unsafe` qualifiers, then reports whether what's left
  // starts a `fn is_*` predicate signature.
  fn starts_is_predicate_fn(trimmed: &str) -> bool {
    let mut rest = trimmed;

    if let Some(after_pub) = rest.strip_prefix("pub") {
      let after_pub = after_pub.trim_start();
      rest = if let Some(after_paren_open) = after_pub.strip_prefix('(') {
        match after_paren_open.find(')') {
          Some(close) => after_paren_open[close + 1..].trim_start(),
          None => after_pub,
        }
      } else {
        after_pub
      };
    }

    loop {
      let mut advanced = false;
      for kw in ["const", "async", "unsafe"] {
        if let Some(after_kw) = rest.strip_prefix(kw)
          && after_kw.starts_with(char::is_whitespace)
        {
          rest = after_kw.trim_start();
          advanced = true;
        }
      }
      if !advanced {
        break;
      }
    }

    rest.starts_with("fn is_")
  }

  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let src_dir = manifest_dir.join("src");

  let mut violations = Vec::new();
  for entry in ignore::WalkBuilder::new(&src_dir)
    .standard_filters(false)
    .build()
    .filter_map(Result::ok)
    .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
  {
    let path = entry.path();
    let Ok(content) = std::fs::read_to_string(path) else {
      continue;
    };
    let lines: Vec<&str> = content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
      let trimmed = line.trim_start();
      if !starts_is_predicate_fn(trimmed) {
        continue;
      }

      // Join the signature across lines (a wrapped multi-line
      // `fn is_foo(\n  ...\n) -> bool {` is common in this crate) up to
      // the opening `{`, or a trailing `;` for a trait-method
      // declaration with no body, whichever comes first.
      let mut header = String::new();
      let mut is_bool_predicate = false;
      for l in lines[i..].iter().take(20) {
        if let Some(pos) = l.find('{') {
          header.push_str(&l[..pos]);
          is_bool_predicate = header.contains("-> bool");
          break;
        }
        let trimmed_end = l.trim_end();
        if let Some(without_semi) = trimmed_end.strip_suffix(';') {
          header.push_str(without_semi);
          is_bool_predicate = header.contains("-> bool");
          break;
        }
        header.push_str(l);
        header.push(' ');
      }
      if !is_bool_predicate {
        continue;
      }

      // Walk upward past any attributes/doc comments other than
      // `#[must_use]` to find whether one is present immediately above
      // the signature (allowing for other attributes in between, e.g.
      // `#[must_use]` then a doc comment is not how this crate writes
      // it, but tolerate ordering rather than over-fitting the scan).
      let has_must_use = lines[..i]
        .iter()
        .rev()
        .take_while(|prior| {
          let t = prior.trim_start();
          t.starts_with('#') || t.starts_with("///") || t.starts_with("//!")
        })
        .any(|prior| prior.trim_start().starts_with("#[must_use]"));

      if !has_must_use {
        violations.push(format!(
          "{}:{}: `{}` is missing `#[must_use]` — see docs/style-guide.md §2",
          path.display(),
          i + 1,
          trimmed
        ));
      }
    }
  }

  assert!(
    violations.is_empty(),
    "predicate-method `#[must_use]` violation(s) — see docs/style-guide.md §2:\n{}",
    violations.join("\n")
  );
}

// Tier-2 enforcement for the rust-guide rule that every file opens with a
// `//!` module-level doc comment (docs/style-guide.md §3 records this test
// and its `tests.rs` exemption), promoted from tier 3
// during #201's QA follow-up [pre-recreation]: a QA review of #201 [pre-recreation] found the rule was
// ~80% unmet across the tree (41 of 50 files at the time) despite the PR
// claiming a clean style-guide sweep, precisely because nothing mechanical
// was checking it. Exempts `tests.rs` sibling files (the §1 directory-
// module test-split exception) the same way an inline `#[cfg(test)] mod
// tests` block is exempt — both are test-only content, not "meaningful
// crate-level content" in the production sense.
#[test]
fn files_carry_module_doc_comment() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let src_dir = manifest_dir.join("src");

  let mut violations = Vec::new();
  for entry in ignore::WalkBuilder::new(&src_dir)
    .standard_filters(false)
    .build()
    .filter_map(Result::ok)
    .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
  {
    let path = entry.path();
    if path.file_name().and_then(|n| n.to_str()) == Some("tests.rs") {
      continue;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
      continue;
    };
    let has_module_doc = content
      .lines()
      .take(15)
      .any(|l| l.trim_start().starts_with("//!"));
    if !has_module_doc {
      violations.push(format!(
        "{}: missing a `//!` module-level doc comment — see docs/style-guide.md §3",
        path.display()
      ));
    }
  }

  assert!(
    violations.is_empty(),
    "module-doc violation(s) — see docs/style-guide.md §3:\n{}",
    violations.join("\n")
  );
}

// Tier-2 enforcement for the `pub mod` doc comment rule documented in
// docs/style-guide.md §3 ("Every `pub mod` declaration ... carries an outer
// `///` doc comment one line above the `mod` keyword describing what the
// module is for").
#[test]
fn pub_mod_declarations_carry_doc_comments() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let src_dir = manifest_dir.join("src");

  let mut violations = Vec::new();
  for entry in ignore::WalkBuilder::new(&src_dir)
    .standard_filters(false)
    .build()
    .filter_map(Result::ok)
    .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
  {
    let path = entry.path();
    let Ok(content) = std::fs::read_to_string(path) else {
      continue;
    };
    let lines: Vec<&str> = content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
      let trimmed = line.trim_start();
      // Check for `pub mod <name>;` or `pub(...) mod <name>;`
      let is_pub_mod = if let Some(after_pub) = trimmed.strip_prefix("pub") {
        let rest = after_pub.trim_start();
        let rest = if let Some(after_paren) = rest.strip_prefix('(') {
          match after_paren.find(')') {
            Some(close) => after_paren[close + 1..].trim_start(),
            None => rest,
          }
        } else {
          rest
        };
        rest.starts_with("mod ") && rest.ends_with(';')
      } else {
        false
      };

      if !is_pub_mod {
        continue;
      }

      // Check if there is an outer doc comment `///` above it
      let has_doc_comment = lines[..i]
        .iter()
        .rev()
        .take_while(|prior| {
          let t = prior.trim_start();
          t.starts_with('#') || t.starts_with("///") || t.starts_with("//!")
        })
        .any(|prior| prior.trim_start().starts_with("///"));

      if !has_doc_comment {
        violations.push(format!(
          "{}:{}: `{}` is missing an outer `///` doc comment — see docs/style-guide.md §3",
          path.display(),
          i + 1,
          trimmed
        ));
      }
    }
  }

  assert!(
    violations.is_empty(),
    "`pub mod` doc comment violation(s) — see docs/style-guide.md §3:\n{}",
    violations.join("\n")
  );
}

// Tier-2 enforcement for canonical module paths rule documented in
// docs/style-guide.md §1 ("new internal code always spells out the canonical,
// structural path (e.g. `crate::ui::table`, `crate::engine::version`) —
// never a crate-root shortcut").
#[test]
fn internal_code_uses_canonical_module_paths() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let src_dir = manifest_dir.join("src");

  let mut violations = Vec::new();
  for entry in ignore::WalkBuilder::new(&src_dir)
    .standard_filters(false)
    .build()
    .filter_map(Result::ok)
    .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
  {
    let path = entry.path();
    // src/lib.rs declares the root re-exports, so it's exempt from checking its own declarations
    if path == src_dir.join("lib.rs") {
      continue;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
      continue;
    };
    for (i, line) in content.lines().enumerate() {
      let trimmed = line.trim_start();
      if trimmed.starts_with("//") {
        continue;
      }
      // Disallow shortcuts like `crate::generate_schema`
      if trimmed.contains("crate::generate_schema") {
        violations.push(format!(
          "{}:{}: uses crate-root re-export shortcut instead of canonical path (use `crate::config::schema::generate_schema`) — see docs/style-guide.md §1",
          path.display(),
          i + 1
        ));
      }
    }
  }

  assert!(
    violations.is_empty(),
    "canonical module path violation(s) — see docs/style-guide.md §1:\n{}",
    violations.join("\n")
  );
}

const ALLOWED_ITEM_IMPORTS: &[&str] = &[
  "clap::CommandFactory",
  "clap::Parser",
  "colored::Colorize",
  "crate::config::facets::DeclaresFacets",
  "crate::config::lang_table::build_resolved_lang_config",
  "crate::config::lang_table::impl_lang_accessors",
  "crate::config::lang_table::impl_lang_merge",
  "crate::config::lang_table::lang_options_table",
  "crate::surfaces::LanguageSurface",
  "crate::surfaces::sync::native::NativeConfig",
  "rayon::iter::IndexedParallelIterator",
  "rayon::iter::IntoParallelRefIterator",
  "rayon::iter::ParallelIterator",
  "serde::Deserialize",
  "serde::Serialize",
  "std::fmt::Write",
  "std::io::BufRead",
  "std::io::Read",
  "std::io::Write",
  "std::os::unix::fs::PermissionsExt",
  "tower_lsp::LanguageServer",
];

const STD_MODULES: &[&str] = &[
  "alloc",
  "any",
  "arch",
  "array",
  "ascii",
  "backtrace",
  "borrow",
  "boxed",
  "cell",
  "char",
  "clone",
  "cmp",
  "collections",
  "convert",
  "default",
  "env",
  "error",
  "ffi",
  "fmt",
  "fs",
  "future",
  "hash",
  "hint",
  "io",
  "iter",
  "marker",
  "mem",
  "net",
  "num",
  "ops",
  "option",
  "os",
  "panic",
  "path",
  "pin",
  "prelude",
  "process",
  "ptr",
  "rc",
  "result",
  "slice",
  "str",
  "string",
  "sync",
  "task",
  "thread",
  "time",
  "vec",
];

// Tier-2 enforcement for module-only imports documented in
// docs/style-guide.md §1 ("every `use` statement in `src/` and `tests/`
// must import a module, never an item. The only permitted exceptions are
// named traits and `use super::*;` inside test modules").
#[test]
fn no_item_imports() {
  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let src_dir = manifest_dir.join("src");
  let tests_dir = manifest_dir.join("tests");

  let mut violations = Vec::new();

  for dir in [&src_dir, &tests_dir] {
    for entry in ignore::WalkBuilder::new(dir)
      .standard_filters(false)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
      .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
    {
      let path = entry.path();
      let Ok(content) = std::fs::read_to_string(path) else {
        continue;
      };

      let is_test_file = path.starts_with(&tests_dir)
        || path.file_name().is_some_and(|n| n == "tests.rs");
      let mut in_test_mod = false;

      for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains("mod tests {") || trimmed.contains("#[cfg(test)]") {
          in_test_mod = true;
        }
        if trimmed.starts_with("//")
          || trimmed.starts_with("/*")
          || trimmed.starts_with('*')
        {
          continue;
        }
        let Some(rest) = trimmed.strip_prefix("use ") else {
          continue;
        };
        let stmt = rest.trim_end_matches(';').trim();

        if stmt == "super::*" {
          if !is_test_file && !in_test_mod {
            violations.push(format!(
              "{}:{}: `use super::*;` outside test context",
              path.display(),
              i + 1
            ));
          }
          continue;
        }

        if stmt.contains('{') || stmt.contains('}') {
          violations.push(format!(
            "{}:{}: grouped use statement `{trimmed}` violates rust-guide §3A",
            path.display(),
            i + 1
          ));
          continue;
        }

        if ALLOWED_ITEM_IMPORTS.contains(&stmt) {
          continue;
        }

        let base_stmt = stmt
          .split_once(" as ")
          .map_or(stmt, |(base, _)| base.trim());
        let segments: Vec<&str> = base_stmt.split("::").collect();
        let last = segments.last().copied().unwrap_or_default();

        if last.chars().next().is_some_and(char::is_uppercase) {
          violations.push(format!(
            "{}:{}: item import `{stmt}` is not an allowed trait — see docs/style-guide.md §1",
            path.display(),
            i + 1
          ));
          continue;
        }

        if segments[0] == "crate" || segments[0] == "fml" {
          // `crate::` inside a `tests/<name>/` crate is that test crate's
          // root directory; everywhere else it is `src/`.
          let mut rel_path = match path.strip_prefix(&tests_dir) {
            Ok(rel) if segments[0] == "crate" => rel
              .components()
              .next()
              .map_or_else(|| tests_dir.clone(), |c| tests_dir.join(c)),
            _ => src_dir.clone(),
          };
          for part in &segments[1..] {
            rel_path.push(part);
          }
          let is_mod = rel_path.with_extension("rs").is_file()
            || rel_path.join("mod.rs").is_file()
            || rel_path.is_dir();
          if !is_mod {
            violations.push(format!(
              "{}:{}: `{stmt}` does not resolve to a module — see docs/style-guide.md §1",
              path.display(),
              i + 1
            ));
          }
        } else if segments[0] == "std"
          && segments.len() > 1
          && !STD_MODULES.contains(&segments[1])
        {
          violations.push(format!(
            "{}:{}: `{stmt}` is not a standard library module — see docs/style-guide.md §1",
            path.display(),
            i + 1
          ));
        }
      }
    }
  }

  assert!(
    violations.is_empty(),
    "item import violation(s) — see docs/style-guide.md §1:\n{}",
    violations.join("\n")
  );
}

// Tier-2 enforcement for pre-recreation issue citations documented in
// docs/INDEX.md ("Note on pre-recreation issue/PR numbers"): source comments
// citing issue numbers from before the 2026-08-26 repository recreation
// must be explicitly disambiguated (e.g. `(Fixes #151 [pre-recreation])` or
// `#120 [pre-recreation]`) so they cannot be mistaken for current issue
// numbers that have since climbed past them and now resolve to real,
// unrelated issues.
#[test]
fn source_files_do_not_contain_bare_pre_recreation_issue_citations() {
  const PRE_RECREATION_NUMBERS: &[u32] = &[
    68, 76, 82, 100, 113, 119, 120, 121, 126, 133, 151, 157, 158, 159, 165,
    177, 191, 192, 194, 195, 201,
  ];

  let manifest_dir = path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let src_dir = manifest_dir.join("src");

  let mut violations = Vec::new();
  for entry in ignore::WalkBuilder::new(&src_dir)
    .standard_filters(false)
    .build()
    .filter_map(Result::ok)
    .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
    .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
  {
    let path = entry.path();
    let Ok(content) = std::fs::read_to_string(path) else {
      continue;
    };

    let rel_path = path
      .strip_prefix(&manifest_dir)
      .unwrap_or(path)
      .to_string_lossy()
      .replace('\\', "/");

    for (i, line) in content.lines().enumerate() {
      // Scan for `#<digits>` pattern in the line.
      let bytes = line.as_bytes();
      let mut idx = 0;
      while idx < bytes.len() {
        if bytes[idx] == b'#' {
          let start = idx + 1;
          let mut end = start;
          while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
          }
          if end > start {
            if let Ok(num) = line[start..end].parse::<u32>()
              && PRE_RECREATION_NUMBERS.contains(&num)
            {
              // Must be marked as pre-recreation unless it's a known post-recreation reference.
              let is_marked = line.contains("pre-recreation");

              // Sanctioned post-recreation references that legitimately share an old number:
              // A module's file or anything in its directory (its tests).
              let in_module = |module: &str| {
                rel_path == format!("{module}.rs")
                  || rel_path.starts_with(&format!("{module}/"))
              };
              let is_sanctioned_post_recreation = match num {
                // Post-recreation #113 is markdownlint-cli2 exit code classification in markdown.rs
                // Post-recreation #120 is disabling MD033/no-inline-html by
                // default in markdown.rs (this fix, not the pre-recreation
                // issue of the same number)
                113 | 120 => in_module("src/surfaces/lang/markdown"),
                // Post-recreation #119 is `fml fix`'s remaining /
                // auto-fixable reporting, split between the runner and
                // its CLI rendering (this fix, not the pre-recreation
                // issue of the same number)
                119 => {
                  in_module("src/engine/runner")
                    || in_module("src/cli/ui/runner")
                    || in_module("src/cli/ui/violations")
                }
                // Post-recreation #157 is path relativization in
                // cli/ui/paths.rs and surfaces/lang/markdown.rs
                157 => {
                  in_module("src/cli/ui/paths")
                    || in_module("src/cli/ui/table")
                    || in_module("src/surfaces/lang/markdown")
                }
                // Post-recreation #177 is version probing model in engine/version/
                // Post-recreation #195 is PR #195 version probing in engine/version/
                177 | 195 => in_module("src/engine/version"),
                // Post-recreation #191 is PR #191 review regression in cli/ui/paths.rs
                191 => in_module("src/cli/ui/paths"),
                // Post-recreation #201 is go lint test runner flakiness fix in go.rs
                201 => in_module("src/surfaces/lang/go"),
                // Post-recreation #151 is prettier-driven --check exit-code classification
                151 => {
                  line.contains("ExecutionError")
                    || line.contains("--check")
                    || line.contains("Fixes #155")
                    || line.contains("ktlint `-F`")
                    || line.contains("Same reasoning applies")
                }
                _ => false,
              };

              if !is_marked && !is_sanctioned_post_recreation {
                violations.push(format!(
                  "{}:{}: unmarked pre-recreation citation `#{}` — mark with `[pre-recreation]` per docs/INDEX.md: `{}`",
                  path.display(),
                  i + 1,
                  num,
                  line.trim()
                ));
              }
            }
            idx = end;
            continue;
          }
        }
        idx += 1;
      }
    }
  }

  assert!(
    violations.is_empty(),
    "bare pre-recreation issue citation(s) — see docs/INDEX.md:\n{}",
    violations.join("\n")
  );
}

// Tier-2 enforcement for the `src/` half of docs/style-guide.md §6's
// exit-status rule (#291): unit tests reach the deciding private function,
// so never dispatch a full command. Scans `tests.rs` files and code after
// `mod tests {`; no runtime assertion can observe what a test calls.
#[test]
fn unit_tests_do_not_dispatch_full_commands() {
  let needle = concat!("run_with", "_args(");
  let mut violations = Vec::new();
  for entry in ignore::WalkBuilder::new(
    path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
  )
  .standard_filters(false)
  .build()
  .filter_map(Result::ok)
  .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
  {
    let path = entry.path();
    let content = std::fs::read_to_string(path).unwrap();
    let start = if path.file_name().is_some_and(|n| n == "tests.rs") {
      0
    } else if let Some(idx) = content.find("mod tests {") {
      content[..idx].matches('\n').count()
    } else {
      continue;
    };
    for (i, line) in content.lines().enumerate().skip(start) {
      let code = line.split("//").next().unwrap_or_default();
      if code.contains(needle) {
        violations.push(format!("{}:{}", path.display(), i + 1));
      }
    }
  }
  assert!(
    violations.is_empty(),
    "unit test dispatches a full command — see docs/style-guide.md §6:\n{}",
    violations.join("\n")
  );
}
