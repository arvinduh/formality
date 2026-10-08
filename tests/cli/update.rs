//! `fml update` replacing the binary it runs from, against a fake release.
//!
//! Each test copies the built `fml` into a scratch directory and points it,
//! through the debug-only `FML_TEST_RELEASES_URL`, at a `file://` release
//! tree, so no network is involved. Only the process shows whether the
//! running executable was replaced, or left byte-identical on a failure.

use std::env;
use std::fmt::Write;
use std::fs;
use std::io;
use std::path;
use std::process;
use std::thread;
use std::time;

use fml::engine::update::install;

/// The version the fake installed `fml` claims, older than any release.
const OLD: &str = "0.0.1";

/// The release tag matching this build's own version.
fn this_tag() -> String {
  format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// A scratch install of `fml` and a fake release tree for it to update from.
struct Fixture {
  dir: tempfile::TempDir,
}

impl Fixture {
  /// Copies the built binary into `<dir>/bin` and publishes `tag` as latest.
  fn new(tag: &str) -> Self {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("bin")).unwrap();
    fs::copy(
      env!("CARGO_BIN_EXE_fml"),
      dir.path().join("bin").join(exe()),
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("releases")).unwrap();
    fs::write(
      dir.path().join("releases").join("latest"),
      format!("{{\"tag_name\":\"{tag}\"}}"),
    )
    .unwrap();
    Self { dir }
  }

  /// The installed binary's path.
  fn bin(&self) -> path::PathBuf {
    self.dir.path().join("bin").join(exe())
  }

  /// Publishes `archive` as this target's asset for `tag`, with `sums` as its
  /// `.sha256` file when given.
  fn publish(&self, tag: &str, archive: &[u8], sums: Option<&str>) {
    let dir = self.dir.path().join("releases").join("download").join(tag);
    fs::create_dir_all(&dir).unwrap();
    let asset = install::asset_name();
    fs::write(dir.join(&asset), archive).unwrap();
    if let Some(sums) = sums {
      fs::write(dir.join(format!("{asset}.sha256")), sums).unwrap();
    }
  }

  /// Packs `payload` as cargo-dist would and publishes it, correctly summed.
  fn publish_binary(&self, tag: &str, payload: &[u8]) {
    let archive = self.pack(payload);
    self.publish(tag, &archive, Some(&sums(&archive)));
  }

  /// Builds this target's archive around `payload` with the system tar.
  fn pack(&self, payload: &[u8]) -> Vec<u8> {
    let stage = self.dir.path().join("pack");
    let archive = self.dir.path().join(install::asset_name());
    let mut tar = if cfg!(windows) {
      fs::create_dir_all(&stage).unwrap();
      fs::write(stage.join(exe()), payload).unwrap();
      let root = env::var_os("SystemRoot").unwrap();
      let mut tar = process::Command::new(
        path::Path::new(&root).join("System32").join("tar.exe"),
      );
      tar.args(["-a", "--options", "zip:compression=store", "-cf"]);
      tar.arg(&archive).arg("-C").arg(&stage).arg(exe());
      tar
    } else {
      let nested = stage.join("fml-target");
      fs::create_dir_all(&nested).unwrap();
      fs::write(nested.join(exe()), payload).unwrap();
      set_executable(&nested.join(exe()));
      let mut tar = process::Command::new("tar");
      tar
        .arg("-cf")
        .arg(&archive)
        .arg("-C")
        .arg(&stage)
        .arg("fml-target");
      tar
    };
    assert!(
      tar.status().unwrap().success(),
      "packing the fixture failed"
    );
    fs::read(archive).unwrap()
  }

  /// Runs the installed `fml update` as if version `current` were installed.
  fn update(&self, current: Option<&str>) -> process::Output {
    let releases = self.dir.path().join("releases");
    let url = format!(
      "file:///{}",
      releases
        .display()
        .to_string()
        .replace('\\', "/")
        .trim_start_matches('/')
    );
    let mut command = process::Command::new(self.bin());
    command
      .arg("update")
      .env("FML_TEST_RELEASES_URL", url)
      .env("NO_COLOR", "1");
    if let Some(current) = current {
      command.env("FML_TEST_CURRENT_VERSION", current);
    }
    // Another test thread forking while this one held the fresh copy open
    // for writing leaves it briefly "text file busy" on Linux; retry that.
    for _ in 0..50 {
      match command.output() {
        Err(err) if err.kind() == io::ErrorKind::ExecutableFileBusy => {
          thread::sleep(time::Duration::from_millis(100));
        }
        output => return output.unwrap(),
      }
    }
    panic!("{} stayed busy", self.bin().display());
  }

  /// Asserts the update failed with `needle` in its error and left the
  /// installed binary, and nothing else, in its directory.
  fn assert_failed_untouched(&self, output: &process::Output, needle: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(needle), "expected `{needle}` in: {stderr}");
    assert_eq!(
      fs::read(self.bin()).unwrap(),
      fs::read(env!("CARGO_BIN_EXE_fml")).unwrap(),
      "a failed update changed the installed binary"
    );
    assert_eq!(
      fs::read_dir(self.dir.path().join("bin")).unwrap().count(),
      1
    );
  }
}

/// The binary's file name on this platform.
fn exe() -> String {
  format!("fml{}", env::consts::EXE_SUFFIX)
}

/// A `.sha256` file for `bytes`, in cargo-dist's format.
fn sums(bytes: &[u8]) -> String {
  let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes);
  let hex = digest.iter().fold(String::new(), |mut hex, byte| {
    write!(hex, "{byte:02x}").unwrap();
    hex
  });
  format!("{hex} *{}\n", install::asset_name())
}

/// A binary that reports `version` and is distinguishable from the installed
/// one: a shell script on Unix, a copy of `fml.exe` with bytes appended on
/// Windows (where it can only report this build's own version).
fn payload(version: &str) -> Vec<u8> {
  if cfg!(windows) {
    let mut bytes = fs::read(env!("CARGO_BIN_EXE_fml")).unwrap();
    bytes.extend_from_slice(b"fml-update-test-payload");
    bytes
  } else {
    format!("#!/bin/sh\necho 'formality {version}'\n").into_bytes()
  }
}

#[cfg(unix)]
fn set_executable(file: &path::Path) {
  use std::os::unix::fs::PermissionsExt;

  fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn set_executable(_: &path::Path) {}

#[test]
fn update_replaces_the_running_binary_in_place() {
  let tag = this_tag();
  let fixture = Fixture::new(&tag);
  let payload = payload(env!("CARGO_PKG_VERSION"));
  fixture.publish_binary(&tag, &payload);

  let output = fixture.update(Some(OLD));
  let stdout = String::from_utf8_lossy(&output.stdout);
  assert_eq!(
    output.status.code(),
    Some(0),
    "{stdout}{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(stdout.contains(&format!("v{OLD} → {tag}")), "{stdout}");
  assert!(stdout.contains("Replaced"), "{stdout}");
  assert_eq!(fs::read(fixture.bin()).unwrap(), payload);
}

#[test]
fn already_latest_exits_zero_and_changes_nothing() {
  let fixture = Fixture::new(&this_tag());
  let output = fixture.update(None);
  let stdout = String::from_utf8_lossy(&output.stdout);
  assert_eq!(output.status.code(), Some(0), "{stdout}");
  assert!(stdout.contains("already the latest release"), "{stdout}");
  assert_eq!(
    fs::read(fixture.bin()).unwrap(),
    fs::read(env!("CARGO_BIN_EXE_fml")).unwrap()
  );
}

#[test]
fn checksum_mismatch_leaves_the_binary_untouched() {
  let tag = this_tag();
  let fixture = Fixture::new(&tag);
  let archive = fixture.pack(&payload(env!("CARGO_PKG_VERSION")));
  fixture.publish(&tag, &archive, Some(&sums(b"something else")));
  fixture.assert_failed_untouched(&fixture.update(Some(OLD)), "sha256");
}

#[test]
fn missing_checksum_file_leaves_the_binary_untouched() {
  let tag = this_tag();
  let fixture = Fixture::new(&tag);
  let archive = fixture.pack(&payload(env!("CARGO_PKG_VERSION")));
  fixture.publish(&tag, &archive, None);
  fixture.assert_failed_untouched(&fixture.update(Some(OLD)), ".sha256");
}

#[test]
fn unreadable_archive_leaves_the_binary_untouched() {
  let tag = this_tag();
  let fixture = Fixture::new(&tag);
  let archive = b"not an archive".to_vec();
  fixture.publish(&tag, &archive, Some(&sums(&archive)));
  fixture.assert_failed_untouched(&fixture.update(Some(OLD)), "tar");
}

#[test]
fn wrong_version_leaves_the_binary_untouched() {
  // The release claims a newer version than the binary inside reports.
  let tag = "v999.0.0";
  let fixture = Fixture::new(tag);
  fixture.publish_binary(tag, &payload(env!("CARGO_PKG_VERSION")));
  fixture.assert_failed_untouched(&fixture.update(Some(OLD)), "reports");
}
