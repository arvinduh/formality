//! One shared renderer for "this spelling is deprecated" notices.
//!
//! `fml` is a published CLI, so a removed command or flag gets one minor
//! release of continuing to work while printing a notice that names its
//! replacement — never a silent removal. Every such notice goes through
//! [`warn_deprecated_spelling`] so the wording, the stream, and the removal
//! version stay identical across commands.
//!
//! Reused by the deprecations that follow this one (#125, #128, #129); do
//! not add a second notice mechanism alongside it.

use colored::Colorize;

/// The release that removes everything currently deprecated.
///
/// Deprecations introduced in `v0.3.0` are removed here. Bump this only
/// together with actually deleting the deprecated spellings.
pub const REMOVAL_VERSION: &str = "v0.4.0";

/// Prints a deprecation notice naming `old`, its `replacement`, and the
/// release that removes it.
///
/// `detail`, when given, is one sentence appended to explain a behavior
/// difference the user should know about before the replacement becomes the
/// only spelling.
///
/// Always written to **stderr**: a deprecated spelling still produces its
/// normal stdout output, and a notice that landed on stdout would corrupt
/// pipelines that consume it.
pub fn warn_deprecated_spelling(
  old: &str,
  replacement: &str,
  detail: Option<&str>,
) {
  let mut message = format!(
    "`{old}` is deprecated and will be removed in {REMOVAL_VERSION}. Use `{replacement}` instead"
  );
  match detail {
    Some(d) => message.push_str(&format!(" — {d}")),
    None => message.push('.'),
  }
  eprintln!("{} {}", "[DEPRECATED]".yellow().bold(), message);
}

#[cfg(test)]
#[allow(missing_docs, clippy::missing_errors_doc, clippy::missing_panics_doc)]
mod tests {
  use super::*;

  #[test]
  fn test_removal_version_is_a_v_prefixed_release() {
    assert!(
      REMOVAL_VERSION.starts_with('v'),
      "removal version should be a tag-shaped release name, got {REMOVAL_VERSION}"
    );
  }

  #[test]
  fn test_warn_deprecated_spelling_does_not_panic_with_and_without_detail() {
    warn_deprecated_spelling("fml lint --fix", "fml fix", None);
    warn_deprecated_spelling(
      "fml lint --fix",
      "fml fix",
      Some("it applies the same lint fixes and then reformats"),
    );
  }
}
