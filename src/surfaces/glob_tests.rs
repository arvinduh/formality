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
