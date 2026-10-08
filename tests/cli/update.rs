//! `fml update` replacing the binary it runs from, against a fake release.
//!
//! Each test copies the built `fml` into a scratch directory and serves a
//! fake GitHub API and installer from a local HTTP server, reached through
//! `axoupdater`'s own `FML_INSTALLER_GHE_BASE_URL`, so no network is
//! involved. The fake installer stands in for cargo-dist's: the real one runs
//! end to end in `install-regression.yml`. Only the process shows whether
//! the running executable was replaced, or left byte-identical on a failure.

use std::env;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::net;
use std::path;
use std::process;
use std::thread;
use std::time;

/// The version the fake installed `fml` claims, older than any release.
const OLD: &str = "0.0.1";

/// What the successful fake installer writes in place of the binary.
const NEW_BINARY: &str = "new fml";

/// A fake installer that replaces `fml` in its forced install directory
/// with [`NEW_BINARY`] and echoes whether it was told not to edit PATH.
fn installer_ok() -> String {
  if cfg!(windows) {
    format!(
      "Write-Output \"FML_NO_MODIFY_PATH=$env:FML_NO_MODIFY_PATH\"\n\
       Set-Content -NoNewline -Path (Join-Path $env:FML_INSTALL_DIR 'fml.exe') \
       -Value '{NEW_BINARY}'\n"
    )
  } else {
    format!(
      "#!/bin/sh\necho \"FML_NO_MODIFY_PATH=$FML_NO_MODIFY_PATH\"\n\
       printf '{NEW_BINARY}' > \"$FML_INSTALL_DIR/fml.new\"\n\
       mv \"$FML_INSTALL_DIR/fml.new\" \"$FML_INSTALL_DIR/fml\"\n"
    )
  }
}

/// A fake installer that fails before installing anything.
fn installer_failing() -> String {
  if cfg!(windows) {
    "Write-Output 'fake installer failed'\nexit 1\n".to_string()
  } else {
    "#!/bin/sh\necho 'fake installer failed' >&2\nexit 1\n".to_string()
  }
}

/// Serves `tag` as the latest release, with `installer` as its installer
/// asset, on a local port; returns the base URL. The server thread lives
/// until the test process exits.
fn serve_release(tag: &str, installer: String) -> String {
  let listener = net::TcpListener::bind("127.0.0.1:0").unwrap();
  let base =
    format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
  let assets = ["fml-installer.sh", "fml-installer.ps1"]
    .map(|name| {
      format!(
        "{{\"name\":\"{name}\",\"url\":\"{base}/installer\",\
         \"browser_download_url\":\"{base}/installer\"}}"
      )
    })
    .join(",");
  let release = format!(
    "{{\"tag_name\":\"{tag}\",\"name\":\"{tag}\",\"url\":\"{base}\",\
     \"prerelease\":false,\"assets\":[{assets}]}}"
  );
  thread::spawn(move || {
    for stream in listener.incoming() {
      let Ok(mut stream) = stream else { continue };
      let _ = respond(&mut stream, &release, &installer);
    }
  });
  base
}

/// Answers one HTTP request: the release JSON, the installer, or a 404.
fn respond(
  stream: &mut net::TcpStream,
  release: &str,
  installer: &str,
) -> io::Result<()> {
  let mut request = Vec::new();
  let mut buf = [0; 1024];
  while !request.windows(4).any(|w| w == b"\r\n\r\n") {
    let n = stream.read(&mut buf)?;
    if n == 0 {
      break;
    }
    request.extend_from_slice(&buf[..n]);
  }
  let request = String::from_utf8_lossy(&request);
  let path = request.split_whitespace().nth(1).unwrap_or("");
  let (status, body) = match path {
    "/api/v3/repos/arvinduh/formality/releases/latest" => ("200 OK", release),
    "/installer" => ("200 OK", installer),
    _ => ("404 Not Found", ""),
  };
  write!(
    stream,
    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
    body.len()
  )
}

/// A scratch install of `fml`.
struct Fixture {
  dir: tempfile::TempDir,
  name: String,
}

impl Fixture {
  /// Copies the built binary into a scratch directory as `name`.
  fn new(name: &str) -> Self {
    let dir = tempfile::tempdir().unwrap();
    fs::copy(env!("CARGO_BIN_EXE_fml"), dir.path().join(name)).unwrap();
    Self {
      dir,
      name: name.to_string(),
    }
  }

  /// The installed binary's path.
  fn bin(&self) -> path::PathBuf {
    self.dir.path().join(&self.name)
  }

  /// Runs `fml update` against `base`, as if version `current` were
  /// installed when given.
  fn update(&self, base: &str, current: Option<&str>) -> process::Output {
    let mut command = process::Command::new(self.bin());
    command
      .arg("update")
      .env("FML_INSTALLER_GHE_BASE_URL", base)
      .env_remove("FML_INSTALLER_GITHUB_BASE_URL")
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

  /// Asserts the installed binary is still the built one, alone in its
  /// directory.
  fn assert_untouched(&self) {
    assert_eq!(
      fs::read(self.bin()).unwrap(),
      fs::read(env!("CARGO_BIN_EXE_fml")).unwrap(),
      "the installed binary changed"
    );
    assert_eq!(fs::read_dir(self.dir.path()).unwrap().count(), 1);
  }
}

/// The binary's file name on this platform.
fn exe() -> String {
  format!("fml{}", env::consts::EXE_SUFFIX)
}

/// The release tag matching this build's own version.
fn this_tag() -> String {
  format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// Both streams of `output`, for assertion messages.
fn printed(output: &process::Output) -> String {
  format!(
    "{}{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  )
}

#[test]
fn update_runs_the_installer_into_the_binarys_own_directory() {
  let base = serve_release(&this_tag(), installer_ok());
  let fixture = Fixture::new(&exe());
  let output = fixture.update(&base, Some(OLD));
  let printed = printed(&output);
  assert_eq!(output.status.code(), Some(0), "{printed}");
  assert!(
    printed.contains(&format!("v{OLD} → {}", this_tag())),
    "{printed}"
  );
  assert!(printed.contains("FML_NO_MODIFY_PATH=1"), "{printed}");
  assert!(printed.contains("Replaced"), "{printed}");
  assert_eq!(fs::read_to_string(fixture.bin()).unwrap(), NEW_BINARY);
}

#[test]
fn installer_failure_leaves_the_binary_untouched() {
  let base = serve_release(&this_tag(), installer_failing());
  let fixture = Fixture::new(&exe());
  let output = fixture.update(&base, Some(OLD));
  let printed = printed(&output);
  assert_eq!(output.status.code(), Some(2), "{printed}");
  assert!(printed.contains("installation failed"), "{printed}");
  fixture.assert_untouched();
}

#[test]
fn already_latest_exits_zero_and_changes_nothing() {
  let base = serve_release(&this_tag(), installer_ok());
  let fixture = Fixture::new(&exe());
  let output = fixture.update(&base, None);
  let printed = printed(&output);
  assert_eq!(output.status.code(), Some(0), "{printed}");
  assert!(printed.contains("already the latest release"), "{printed}");
  fixture.assert_untouched();
}

#[test]
fn renamed_binary_is_refused_before_the_installer_runs() {
  let base = serve_release(&this_tag(), installer_ok());
  let fixture = Fixture::new(&format!("fml-dev{}", env::consts::EXE_SUFFIX));
  let output = fixture.update(&base, Some(OLD));
  let printed = printed(&output);
  assert_eq!(output.status.code(), Some(2), "{printed}");
  assert!(printed.contains("is not named fml"), "{printed}");
  fixture.assert_untouched();
}
