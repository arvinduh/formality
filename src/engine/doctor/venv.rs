//! Virtual environment detection for the Python surface in `fml doctor`.
//!
//! Inspects active Python virtual environments. Git ignore checks are owned by
//! `super::gitignore`.

use std::path;

/// Indicates the origin of a detected Python virtual environment.
#[derive(Debug, PartialEq)]
pub enum VirtualEnvSource {
  /// Virtual environment specified via `VIRTUAL_ENV` environment variable.
  EnvVar,
  /// Virtual environment directory discovered within the workspace.
  Workspace(String),
  /// No virtual environment detected.
  None,
}

/// Metadata about a detected Python virtual environment.
#[derive(Debug)]
pub struct VirtualEnvInfo {
  /// Whether the virtual environment is currently active.
  pub is_active: bool,
  /// Path to the virtual environment directory, if present.
  pub venv_path: Option<path::PathBuf>,
  /// Path to the resolved Python interpreter executable, if present.
  pub interpreter_path: Option<path::PathBuf>,
  /// Source mechanism through which the virtual environment was detected.
  pub source: VirtualEnvSource,
}

/// Look for Python interpreter binary inside a virtual environment directory.
#[must_use]
pub fn find_venv_interpreter(venv_path: &path::Path) -> Option<path::PathBuf> {
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

/// Finds the system Python interpreter binary on PATH (`python3` or `python`).
#[must_use]
pub fn find_system_python() -> Option<path::PathBuf> {
  which::which("python3")
    .or_else(|_| which::which("python"))
    .ok()
}

/// Detects active virtual environment (via `VIRTUAL_ENV`) or workspace virtualenv directory (`.venv`, `venv`, `env`, `.env`).
pub fn detect_virtualenv(root: &path::Path) -> VirtualEnvInfo {
  detect_virtualenv_with_env(
    root,
    std::env::var_os("VIRTUAL_ENV").map(path::PathBuf::from),
  )
}

/// Detects virtual environment status given optional explicit `VIRTUAL_ENV` path.
#[must_use]
pub fn detect_virtualenv_with_env(
  root: &path::Path,
  env_var: Option<path::PathBuf>,
) -> VirtualEnvInfo {
  if let Some(venv_dir) = env_var.filter(|p| !p.as_os_str().is_empty()) {
    let interpreter =
      find_venv_interpreter(&venv_dir).or_else(find_system_python);
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
      let interpreter = find_venv_interpreter(&dir).or_else(find_system_python);
      return VirtualEnvInfo {
        is_active: false,
        venv_path: Some(dir),
        interpreter_path: interpreter,
        source: VirtualEnvSource::Workspace(dir_name.to_string()),
      };
    }
  }

  let sys_interpreter = find_system_python();
  VirtualEnvInfo {
    is_active: false,
    venv_path: None,
    interpreter_path: sys_interpreter,
    source: VirtualEnvSource::None,
  }
}
