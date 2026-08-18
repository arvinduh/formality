use super::*;
use crate::surfaces::SurfaceStatus;
use tempfile::TempDir;

fn create_dummy_success_output() -> std::process::Output {
  #[cfg(windows)]
  {
    std::process::Command::new("cmd")
      .args(["/C", "exit 0"])
      .output()
      .expect("cmd exit 0 failed")
  }
  #[cfg(not(windows))]
  {
    std::process::Command::new("true")
      .output()
      .expect("true failed")
  }
}

#[test]
fn test_diff_check_via_tempcopy_clean() {
  let temp = TempDir::new().unwrap();
  let file = temp.path().join("clean.rs");
  std::fs::write(&file, "fn main() {\n  println!(\"clean\");\n}\n").unwrap();

  let start = Instant::now();
  let res = diff_check_via_tempcopy(
    std::slice::from_ref(&file),
    |_scratch| Ok(create_dummy_success_output()),
    "rust",
    start,
  );

  assert!(matches!(res.status, SurfaceStatus::Passed));

  let ext = file.extension().unwrap().to_str().unwrap();
  let file_stem = file.file_stem().unwrap().to_str().unwrap();
  let scratch =
    file.with_file_name(format!("{}.fml-check-tmp.{}", file_stem, ext));
  assert!(!scratch.exists());
}

#[test]
fn test_diff_check_via_tempcopy_with_diff() {
  let temp = TempDir::new().unwrap();
  let file = temp.path().join("dirty.rs");
  std::fs::write(&file, "fn main() {let x=1;}").unwrap();

  let start = Instant::now();
  let res = diff_check_via_tempcopy(
    std::slice::from_ref(&file),
    |scratch| {
      std::fs::write(scratch, "fn main() {\n  let x = 1;\n}\n")?;
      Ok(create_dummy_success_output())
    },
    "rust",
    start,
  );

  match res.status {
    SurfaceStatus::ViolationsFound { message, diff } => {
      assert!(message.is_empty());
      let diff_str = diff.expect("diff should be present");
      assert!(diff_str.contains("dirty.rs"));
      assert!(diff_str.contains("(formatted)"));
    }
    other => panic!("Expected ViolationsFound, got {:?}", other),
  }

  let ext = file.extension().unwrap().to_str().unwrap();
  let file_stem = file.file_stem().unwrap().to_str().unwrap();
  let scratch =
    file.with_file_name(format!("{}.fml-check-tmp.{}", file_stem, ext));
  assert!(!scratch.exists());
}

#[test]
fn test_diff_check_via_tempcopy_raii_cleanup_on_error() {
  let temp = TempDir::new().unwrap();
  let file = temp.path().join("error_case.rs");
  std::fs::write(&file, "invalid syntax").unwrap();

  let start = Instant::now();
  let res = diff_check_via_tempcopy(
    std::slice::from_ref(&file),
    |_scratch| Err(std::io::Error::other("mock execution error")),
    "rust",
    start,
  );

  assert!(matches!(res.status, SurfaceStatus::ExecutionError { .. }));

  let ext = file.extension().unwrap().to_str().unwrap();
  let file_stem = file.file_stem().unwrap().to_str().unwrap();
  let scratch =
    file.with_file_name(format!("{}.fml-check-tmp.{}", file_stem, ext));
  assert!(!scratch.exists());
}

#[test]
fn test_diff_check_via_tempcopy_raii_cleanup_on_panic() {
  let temp = TempDir::new().unwrap();
  let file = temp.path().join("panic_case.rs");
  std::fs::write(&file, "panic content").unwrap();

  let start = Instant::now();
  let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    diff_check_via_tempcopy(
      std::slice::from_ref(&file),
      |_scratch| {
        panic!("simulated panic inside run_in_place");
      },
      "rust",
      start,
    );
  }));

  let ext = file.extension().unwrap().to_str().unwrap();
  let file_stem = file.file_stem().unwrap().to_str().unwrap();
  let scratch =
    file.with_file_name(format!("{}.fml-check-tmp.{}", file_stem, ext));
  assert!(!scratch.exists());
}
