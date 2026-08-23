//! Virtual environment detection for the Python surface.

use std::path::{Path, PathBuf};

/// Indicates the origin of a detected Python virtual environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualEnvSource {
  /// Virtual environment specified via `VIRTUAL_ENV` environment variable.
  EnvVar,
  /// Virtual environment directory discovered within the workspace.
  Workspace(String),
  /// No virtual environment detected.
  None,
}

/// Metadata about a detected Python virtual environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualEnvInfo {
  /// Whether the virtual environment is currently active.
  pub is_active: bool,
  /// Path to the virtual environment directory, if present.
  pub venv_path: Option<PathBuf>,
  /// Path to the resolved Python interpreter executable, if present.
  pub interpreter_path: Option<PathBuf>,
  /// Source mechanism through which the virtual environment was detected.
  pub source: VirtualEnvSource,
}

/// Look for Python interpreter binary inside a virtual environment directory.
#[must_use]
pub fn find_venv_interpreter(venv_path: &Path) -> Option<PathBuf> {
  let candidates = [
    venv_path.join("Scripts").join("python.exe"),
    venv_path.join("Scripts").join("python"),
    venv_path.join("bin").join("python"),
    venv_path.join("bin").join("python3"),
    venv_path.join("bin").join("python.exe"),
    venv_path.join("python.exe"),
    venv_path.join("python"),
  ];
  for candidate in &candidates {
    if candidate.is_file() {
      return Some(candidate.clone());
    }
  }
  None
}

/// Detects active virtual environment (via `VIRTUAL_ENV`) or workspace virtualenv directory (`.venv`, `venv`, `env`, `.env`).
pub fn detect_virtualenv(root: &Path) -> VirtualEnvInfo {
  detect_virtualenv_with_env(
    root,
    std::env::var_os("VIRTUAL_ENV").map(PathBuf::from),
  )
}

/// Detects virtual environment status given optional explicit `VIRTUAL_ENV` path.
#[must_use]
pub fn detect_virtualenv_with_env(
  root: &Path,
  env_var: Option<PathBuf>,
) -> VirtualEnvInfo {
  if let Some(venv_dir) = env_var.filter(|p| !p.as_os_str().is_empty()) {
    let interpreter = find_venv_interpreter(&venv_dir).or_else(|| {
      which::which("python3")
        .or_else(|_| which::which("python"))
        .ok()
    });
    return VirtualEnvInfo {
      is_active: true,
      venv_path: Some(venv_dir),
      interpreter_path: interpreter,
      source: VirtualEnvSource::EnvVar,
    };
  }

  let candidates = [".venv", "venv", "env", ".env"];
  for dir_name in candidates {
    let dir = root.join(dir_name);
    if dir.is_dir() {
      let interpreter = find_venv_interpreter(&dir).or_else(|| {
        which::which("python3")
          .or_else(|_| which::which("python"))
          .ok()
      });
      return VirtualEnvInfo {
        is_active: false,
        venv_path: Some(dir),
        interpreter_path: interpreter,
        source: VirtualEnvSource::Workspace(dir_name.to_string()),
      };
    }
  }

  let sys_interpreter = which::which("python3")
    .or_else(|_| which::which("python"))
    .ok();
  VirtualEnvInfo {
    is_active: false,
    venv_path: None,
    interpreter_path: sys_interpreter,
    source: VirtualEnvSource::None,
  }
}
