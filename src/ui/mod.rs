//! Terminal UI helpers: semantic table formatting and rendering.

/// Semantic table layout and rendering components.
pub mod table;

/// One shared relative-vs-absolute rendering for filesystem paths in output.
pub mod paths;

/// Returns whether the environment explicitly asks for *no* color, per the
/// `NO_COLOR` convention (<https://no-color.org>): the variable set to any
/// non-empty value. Empty means "unset" there, so `NO_COLOR=` deliberately
/// does not disable color.
///
/// Takes precedence over [`color_forced`] everywhere color is decided --
/// both in [`table::Palette::detect`] for this crate's own escape codes and
/// in the global `colored` override set at startup, which is why it lives
/// here rather than being re-checked ad hoc at each site.
#[must_use]
pub fn no_color_requested() -> bool {
  std::env::var("NO_COLOR").is_ok_and(|value| !value.is_empty())
}

/// Returns whether the environment asks for color even though stdout may
/// not be a TTY: `FORCE_COLOR`, `CLICOLOR_FORCE`, or running under GitHub
/// Actions (whose log viewer renders ANSI but whose steps are not TTYs).
#[must_use]
pub fn color_forced() -> bool {
  std::env::var("FORCE_COLOR").is_ok()
    || std::env::var("CLICOLOR_FORCE").is_ok()
    || std::env::var("GITHUB_ACTIONS").is_ok()
}
