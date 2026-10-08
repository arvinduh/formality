//! Installing a published release over the running `fml` binary.
//!
//! Owns `fml update`'s steps, starting with naming this build's cargo-dist
//! archive. The background release check and its cache live in the parent
//! `update` module; the CLI decides what the user sees.

/// The target triple this binary was compiled for (set by `build.rs`).
const TARGET: &str = env!("FML_TARGET");

/// Returns the cargo-dist archive name for this build's target, such as
/// `fml-x86_64-unknown-linux-gnu.tar.xz`. cargo-dist zips Windows builds and
/// packs every other target as `.tar.xz`.
#[must_use]
pub fn asset_name() -> String {
  let extension = if cfg!(windows) { "zip" } else { "tar.xz" };
  format!("fml-{TARGET}.{extension}")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn asset_name_follows_cargo_dist_naming_for_this_target() {
    let name = asset_name();
    assert!(name.starts_with(&format!("fml-{TARGET}.")), "{name}");
    assert_eq!(
      name.rsplit('.').next() == Some("zip"),
      cfg!(windows),
      "{name}"
    );
  }
}
