//! Which files a run acts on when it asks git: `--staged` and `--changed`.
//!
//! Owns the git commands and filtering their files by the paths the user
//! named. Grouping by language is `discover`'s job.

use std::path;
use std::process;

/// Which of git's files to act on.
#[derive(Clone, Copy)]
pub enum Git {
  /// Files in the index.
  Staged,
  /// Files that differ from `HEAD`, staged or not, plus untracked files.
  Changed,
}

/// The files `which` selects under `root`, kept only where they are one of
/// `paths` or under one, when `paths` is not empty. Deleted files are left
/// out.
///
/// # Errors
///
/// Returns an error when git cannot run or fails.
pub fn files(
  root: &path::Path,
  which: Git,
  paths: &[path::PathBuf],
) -> Result<Vec<path::PathBuf>, crate::Error> {
  let mut names = match which {
    Git::Staged => git(root, &["diff", "--name-only", "--cached"])?,
    Git::Changed
      if git(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok() =>
    {
      let mut names = git(root, &["diff", "--name-only", "HEAD"])?;
      names.extend(git(root, &["ls-files", "--others", "--exclude-standard"])?);
      names
    }
    // With no commit yet, every file in the index or untracked is new.
    Git::Changed => git(
      root,
      &["ls-files", "--cached", "--others", "--exclude-standard"],
    )?,
  };
  names.sort();
  names.dedup();
  Ok(
    names
      .into_iter()
      .map(|name| root.join(name))
      .filter(|file| file.is_file())
      .filter(|file| {
        paths.is_empty() || paths.iter().any(|path| file.starts_with(path))
      })
      .collect(),
  )
}

/// Runs git with `args` in `root`, limited to files under `root`, and
/// returns the lines it prints, relative to `root`.
fn git(root: &path::Path, args: &[&str]) -> Result<Vec<String>, crate::Error> {
  let mut command = process::Command::new("git");
  // Unquoted, so a non-ASCII name comes back as itself.
  command
    .current_dir(root)
    .args(["-c", "core.quotePath=false"])
    .args(args);
  if args[0] == "diff" {
    command.args(["--relative", "--diff-filter=d"]);
  }
  let output = command.output().map_err(|source| crate::Error::Git {
    message: source.to_string(),
  })?;
  if !output.status.success() {
    return Err(crate::Error::Git {
      message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    });
  }
  Ok(
    String::from_utf8_lossy(&output.stdout)
      .lines()
      .filter(|line| !line.is_empty())
      .map(str::to_owned)
      .collect(),
  )
}
