//! Build script: records the target triple the binary is compiled for.
//!
//! `fml update` downloads the cargo-dist archive named after this triple, so
//! the compiler's own `TARGET` is the source of truth rather than a runtime
//! OS/arch guess; `engine::update::install` reads it as `FML_TARGET`.

use std::env;

fn main() {
  let target = env::var("TARGET").expect("cargo sets TARGET for build scripts");
  println!("cargo:rustc-env=FML_TARGET={target}");
}
