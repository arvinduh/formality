//! Language surface driver modules for all 12 supported ecosystems.
//!
//! Exposes implementations of [`super::LanguageSurface`] for individual language tooling.
//! Fleet registration and detection are owned by [`super::registry`].

/// C and C++ via `clang-format` and `clang-tidy`.
pub mod cpp;
/// Go via `gofmt`, `goimports`, and `golangci-lint`.
pub mod go;
/// Java via `google-java-format` and `checkstyle`.
pub mod java;
/// JavaScript and TypeScript via `biome`.
pub mod javascript;
/// JSON via `prettier`.
pub mod json;
/// Kotlin via `ktlint`.
pub mod kotlin;
/// Markdown via `prettier` and `markdownlint-cli2`.
pub mod markdown;
/// Python via `ruff`.
pub mod python;
/// Rust via `rustfmt` and `clippy`.
pub mod rust;
/// TOML via `taplo`.
pub mod toml;
/// Typst via `typstyle`.
pub mod typst;
/// YAML via `prettier` and `yamllint`.
pub mod yaml;
