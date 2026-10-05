//! JSON Schema generation for `formality.toml`.
//!
//! Produces the JSON Schema specification from [`super::FormalityConfig`].
//! Parsing and runtime validation of configuration files are owned by [`super::strict`].

use schemars;
use serde_json;

use crate::config;

/// Generates the JSON Schema for formality configuration dynamically using schemars.
#[must_use]
pub fn generate_schema() -> String {
  let schema = schemars::schema_for!(config::FormalityConfig);
  serde_json::to_string_pretty(&schema).unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_generate_schema_valid_json() {
    let schema_str = generate_schema();
    assert!(!schema_str.is_empty());
    let parsed: serde_json::Value =
      serde_json::from_str(&schema_str).expect("Valid JSON schema");
    assert_eq!(parsed["title"], "FormalityConfig");
    assert!(parsed.get("properties").is_some());
  }
}
