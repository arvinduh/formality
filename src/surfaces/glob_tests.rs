use super::*;
use std::path::PathBuf;

#[test]
fn test_find_files_with_ext_files_override() {
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();
  let file_a = root.join("a.rs");
  let file_b = root.join("b.rs");
  let file_c = root.join("c.rs");
  std::fs::write(&file_a, "fn a() {}").unwrap();
  std::fs::write(&file_b, "fn b() {}").unwrap();
  std::fs::write(&file_c, "fn c() {}").unwrap();

  let files_override = vec![PathBuf::from("a.rs"), PathBuf::from("c.rs")];
  let matched = find_files_with_ext(root, &["rs"], &[], &files_override, &[]);
  assert_eq!(matched.len(), 2);
  assert!(matched.contains(&file_a));
  assert!(matched.contains(&file_c));
  assert!(!matched.contains(&file_b));
}

#[test]
fn test_find_files_with_ext_exclude_patterns() {
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();
  let src_dir = root.join("src");
  let gen_dir = src_dir.join("generated");
  std::fs::create_dir_all(&gen_dir).unwrap();

  let normal = src_dir.join("main.rs");
  let generated = gen_dir.join("api.rs");
  let ignored = src_dir.join("ignored.rs");
  std::fs::write(&normal, "fn main() {}").unwrap();
  std::fs::write(&generated, "fn api() {}").unwrap();
  std::fs::write(&ignored, "fn ignored() {}").unwrap();

  let exclude =
    vec![PathBuf::from("src/generated"), PathBuf::from("ignored.rs")];
  let matched = find_files_with_ext(root, &["rs"], &[], &[], &exclude);
  assert_eq!(matched.len(), 1);
  assert_eq!(matched[0], normal);
}

#[test]
fn test_find_files_with_ext_specific_paths_precedence() {
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();
  let file_a = root.join("a.rs");
  let file_b = root.join("b.rs");
  std::fs::write(&file_a, "fn a() {}").unwrap();
  std::fs::write(&file_b, "fn b() {}").unwrap();

  let specific = vec![PathBuf::from("a.rs")];
  let files_override = vec![PathBuf::from("b.rs")];
  let matched =
    find_files_with_ext(root, &["rs"], &specific, &files_override, &[]);
  assert_eq!(matched.len(), 1);
  assert_eq!(matched[0], file_a);
}

#[test]
fn test_find_files_with_ext_default_walk_finds_nested_files() {
  // The default walk_dir_ext path (no specific_paths, no files_override) —
  // previously only exercised indirectly through the exclude test, never
  // asserted on its own for a plain recursive directory walk.
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();
  let nested = root.join("src").join("nested");
  std::fs::create_dir_all(&nested).unwrap();

  let top = root.join("main.rs");
  let deep = nested.join("deep.rs");
  let wrong_ext = root.join("readme.md");
  std::fs::write(&top, "fn main() {}").unwrap();
  std::fs::write(&deep, "fn deep() {}").unwrap();
  std::fs::write(&wrong_ext, "# readme").unwrap();

  let matched = find_files_with_ext(root, &["rs"], &[], &[], &[]);
  assert_eq!(matched.len(), 2);
  assert!(matched.contains(&top));
  assert!(matched.contains(&deep));
  assert!(!matched.contains(&wrong_ext));
}

#[test]
fn test_walk_dir_ext_skips_conventional_ignored_directories() {
  // walk_dir_ext's filter_entry excludes target/, node_modules/, .git/,
  // .venv/, vendor/, and fixtures/ by name — none of these exclusions had
  // any test coverage, so a regression here (e.g. a typo'd directory name)
  // would silently start scanning build artifacts / vendored deps.
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();

  let real = root.join("src");
  std::fs::create_dir_all(&real).unwrap();
  std::fs::write(real.join("lib.rs"), "fn lib() {}").unwrap();

  for ignored_dir in ["target", "node_modules", ".venv", "vendor", "fixtures"] {
    let dir = root.join(ignored_dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("should_not_be_found.rs"), "fn x() {}").unwrap();
  }

  let matched = find_files_with_ext(root, &["rs"], &[], &[], &[]);
  assert_eq!(
    matched.len(),
    1,
    "only src/lib.rs should be found; ignored dirs must be skipped: {matched:?}"
  );
  assert!(matched[0].ends_with("lib.rs"));
}

#[test]
fn test_is_excluded_standalone_function() {
  // is_excluded is the public single-path variant of the normalized
  // exclude-matching machinery find_files_with_ext uses internally; it had
  // no direct test of its own.
  let temp = tempfile::TempDir::new().unwrap();
  let root = temp.path();
  let excluded_file = root.join("build").join("out.rs");
  let kept_file = root.join("src").join("main.rs");

  let exclude = vec![PathBuf::from("build")];
  assert!(is_excluded(&excluded_file, root, &exclude));
  assert!(!is_excluded(&kept_file, root, &exclude));

  // An empty exclude list never excludes anything.
  assert!(!is_excluded(&excluded_file, root, &[]));
}

#[test]
fn test_simple_glob_match() {
  assert!(simple_glob_match("*.rs", "main.rs"));
  assert!(!simple_glob_match("*.rs", "src/main.rs"));
  assert!(!simple_glob_match("*.rs", "src\\main.rs"));
  assert!(simple_glob_match("src/*.rs", "src/main.rs"));
  assert!(simple_glob_match("src/*.rs", "src/lib.rs"));
  assert!(simple_glob_match("src/*.rs", "src\\lib.rs"));
  assert!(simple_glob_match("src\\*.rs", "src/lib.rs"));
  assert!(!simple_glob_match("src/*.rs", "src/sub/lib.rs"));
  assert!(!simple_glob_match("src/*.rs", "src\\sub\\lib.rs"));
  assert!(simple_glob_match("src/**/*.rs", "src/lib.rs"));
  assert!(simple_glob_match("src/**/*.rs", "src\\lib.rs"));
  assert!(simple_glob_match("src/**/*.rs", "src/sub/lib.rs"));
  assert!(simple_glob_match("src/**/*.rs", "src\\sub\\lib.rs"));
  assert!(simple_glob_match("src/**/*.rs", "src/gen/api.rs"));
  assert!(simple_glob_match("src/**/api.rs", "src/gen/api.rs"));
  assert!(simple_glob_match("*.toml", "Cargo.toml"));
  assert!(!simple_glob_match("*.toml", "src/Cargo.toml"));
  assert!(simple_glob_match("target/*", "target/debug"));
  assert!(simple_glob_match("target/*", "target\\debug"));
  assert!(!simple_glob_match("target/*", "target/debug/app"));
  assert!(!simple_glob_match("target/*", "target\\debug\\app"));
  assert!(simple_glob_match("target/**", "target/debug/app"));
  assert!(simple_glob_match("target/**", "target\\debug\\app"));
  assert!(simple_glob_match("**/*.rs", "main.rs"));
  assert!(simple_glob_match("**/*.rs", "src/lib.rs"));
  assert!(simple_glob_match("**/*.rs", "src/sub/lib.rs"));
  assert!(simple_glob_match("test?.rs", "test1.rs"));
  assert!(!simple_glob_match("*.py", "main.rs"));
  assert!(!simple_glob_match("test?.rs", "test12.rs"));
  assert!(!simple_glob_match("test?.rs", "test/a.rs"));
}
