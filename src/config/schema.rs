use crate::config::FormalityConfig;

/// Generates the JSON Schema for formality configuration dynamically using schemars.
pub fn generate_schema() -> String {
  let schema = schemars::schema_for!(FormalityConfig);
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
