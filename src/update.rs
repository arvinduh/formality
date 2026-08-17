use colored::Colorize;

pub fn check_for_updates() {
  // Suppress update checks in CI/CD environments
  if std::env::var("CI").is_ok()
    || std::env::var("GITHUB_ACTIONS").is_ok()
    || std::env::var("FORMALITY_NO_UPDATE_CHECK").is_ok()
  {
    return;
  }

  // Fast non-blocking / timeout check using curl or standard command if available
  let _ = std::thread::spawn(|| {
    let current_version = env!("CARGO_PKG_VERSION");
    if let Ok(output) = std::process::Command::new("curl")
      .args([
        "-s",
        "--connect-timeout",
        "1",
        "--max-time",
        "2",
        "-H",
        "User-Agent: formality-cli",
        "https://api.github.com/repos/arvinduh/formality/releases/latest",
      ])
      .output()
      && output.status.success()
    {
      let body = String::from_utf8_lossy(&output.stdout);
      if let Some(tag_line) = body.lines().find(|l| l.contains("\"tag_name\":"))
      {
        let latest_tag = tag_line
          .split(':')
          .nth(1)
          .unwrap_or("")
          .trim()
          .trim_matches(|c| c == '"' || c == ',' || c == ' ');

        let clean_latest = latest_tag.trim_start_matches('v');
        if !clean_latest.is_empty() && clean_latest != current_version {
          eprintln!(
            "\n{} A new version of formality is available: {} (current: {})\n   Update via: {}",
            "⚡".yellow().bold(),
            latest_tag.green().bold(),
            format!("v{}", current_version).dimmed(),
            "cargo install --git https://github.com/arvinduh/formality".cyan()
          );
        }
      }
    }
  });
}
