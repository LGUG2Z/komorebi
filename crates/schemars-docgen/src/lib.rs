#![warn(clippy::all)]

//! # schemars-docgen
//!
//! Generate MDX documentation from `schemars::Schema` objects.
//!
//! This library takes a JSON Schema (as produced by schemars) and generates
//! MDX documentation suitable for Astro Starlight with:
//!
//! - YAML frontmatter
//! - Starlight components
//! - Markdown rendering for descriptions
//!
//! ## Usage
//!
//! ```rust,no_run
//! use schemars::schema_for;
//! use schemars_docgen::{generate_mdx, MdxOptions};
//!
//! #[derive(schemars::JsonSchema)]
//! struct MyConfig {
//!     /// The name of the configuration
//!     name: String,
//! }
//!
//! let schema = schema_for!(MyConfig);
//! let options = MdxOptions::default();
//! let mdx = generate_mdx(&schema, &options).unwrap();
//! ```

mod mdx_generator;
mod prepare;

pub use mdx_generator::MdxGenerator;

use schemars::Schema;
use serde_json::Value;

/// Options for MDX generation
#[derive(Debug, Clone, Default)]
pub struct MdxOptions {
    /// Override the schema title
    pub title_override: Option<String>,
    /// Override the schema description (used in frontmatter)
    pub description_override: Option<String>,
}

/// Error type for documentation generation
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Template error: {0}")]
    Template(#[from] tera::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Generate MDX documentation from a schema
///
/// This generates an MDX document suitable for use with Astro Starlight.
/// The output includes YAML frontmatter and uses Starlight components.
pub fn generate_mdx(schema: &Schema, options: &MdxOptions) -> Result<String, Error> {
    let generator = MdxGenerator::new()?;
    generator.generate(schema, options)
}

/// Extension trait for convenient access to common schema properties
///
/// Since `schemars::Schema` is a wrapper around `serde_json::Value`,
/// this trait provides typed accessors for standard JSON Schema keywords.
pub trait SchemaExt {
    /// Get the schema title
    fn title(&self) -> Option<&str>;

    /// Get the schema description
    fn description(&self) -> Option<&str>;

    /// Get the schema type (can be a string or array of strings)
    fn schema_type(&self) -> Option<&Value>;

    /// Get object properties
    fn properties(&self) -> Option<&serde_json::Map<String, Value>>;

    /// Get schema definitions ($defs)
    fn definitions(&self) -> Option<&serde_json::Map<String, Value>>;

    /// Get array items schema
    fn items(&self) -> Option<&Value>;

    /// Get anyOf schemas
    fn any_of(&self) -> Option<&Vec<Value>>;

    /// Get oneOf schemas
    fn one_of(&self) -> Option<&Vec<Value>>;

    /// Get allOf schemas
    fn all_of(&self) -> Option<&Vec<Value>>;

    /// Get enum values
    fn enum_values(&self) -> Option<&Vec<Value>>;

    /// Get $ref target
    fn ref_target(&self) -> Option<&str>;

    /// Get default value
    fn default_value(&self) -> Option<&Value>;

    /// Get required fields
    fn required_fields(&self) -> Option<&Vec<Value>>;

    /// Get format constraint
    fn format(&self) -> Option<&str>;

    /// Get pattern constraint
    fn pattern(&self) -> Option<&str>;

    /// Get minimum constraint
    fn minimum(&self) -> Option<f64>;

    /// Get maximum constraint
    fn maximum(&self) -> Option<f64>;

    /// Get const value
    fn const_value(&self) -> Option<&Value>;

    /// Get pattern properties
    fn pattern_properties(&self) -> Option<&serde_json::Map<String, Value>>;

    /// Get additional properties schema
    fn additional_properties(&self) -> Option<&Value>;

    /// Get prefix items (tuple validation)
    fn prefix_items(&self) -> Option<&Vec<Value>>;

    /// Get min items constraint
    fn min_items(&self) -> Option<u64>;

    /// Get max items constraint
    fn max_items(&self) -> Option<u64>;

    /// Check if deprecated
    fn is_deprecated(&self) -> bool;
}

impl SchemaExt for Schema {
    fn title(&self) -> Option<&str> {
        self.as_object()?.get("title")?.as_str()
    }

    fn description(&self) -> Option<&str> {
        self.as_object()?.get("description")?.as_str()
    }

    fn schema_type(&self) -> Option<&Value> {
        self.as_object()?.get("type")
    }

    fn properties(&self) -> Option<&serde_json::Map<String, Value>> {
        self.as_object()?.get("properties")?.as_object()
    }

    fn definitions(&self) -> Option<&serde_json::Map<String, Value>> {
        self.as_object()?.get("$defs")?.as_object()
    }

    fn items(&self) -> Option<&Value> {
        self.as_object()?.get("items")
    }

    fn any_of(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("anyOf")?.as_array()
    }

    fn one_of(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("oneOf")?.as_array()
    }

    fn all_of(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("allOf")?.as_array()
    }

    fn enum_values(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("enum")?.as_array()
    }

    fn ref_target(&self) -> Option<&str> {
        self.as_object()?.get("$ref")?.as_str()
    }

    fn default_value(&self) -> Option<&Value> {
        self.as_object()?.get("default")
    }

    fn required_fields(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("required")?.as_array()
    }

    fn format(&self) -> Option<&str> {
        self.as_object()?.get("format")?.as_str()
    }

    fn pattern(&self) -> Option<&str> {
        self.as_object()?.get("pattern")?.as_str()
    }

    fn minimum(&self) -> Option<f64> {
        self.as_object()?.get("minimum")?.as_f64()
    }

    fn maximum(&self) -> Option<f64> {
        self.as_object()?.get("maximum")?.as_f64()
    }

    fn const_value(&self) -> Option<&Value> {
        self.as_object()?.get("const")
    }

    fn pattern_properties(&self) -> Option<&serde_json::Map<String, Value>> {
        self.as_object()?.get("patternProperties")?.as_object()
    }

    fn additional_properties(&self) -> Option<&Value> {
        self.as_object()?.get("additionalProperties")
    }

    fn prefix_items(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("prefixItems")?.as_array()
    }

    fn min_items(&self) -> Option<u64> {
        self.as_object()?.get("minItems")?.as_u64()
    }

    fn max_items(&self) -> Option<u64> {
        self.as_object()?.get("maxItems")?.as_u64()
    }

    fn is_deprecated(&self) -> bool {
        self.as_object()
            .and_then(|o| o.get("deprecated"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

/// Extension trait for Value to use the same accessors
impl SchemaExt for Value {
    fn title(&self) -> Option<&str> {
        self.as_object()?.get("title")?.as_str()
    }

    fn description(&self) -> Option<&str> {
        self.as_object()?.get("description")?.as_str()
    }

    fn schema_type(&self) -> Option<&Value> {
        self.as_object()?.get("type")
    }

    fn properties(&self) -> Option<&serde_json::Map<String, Value>> {
        self.as_object()?.get("properties")?.as_object()
    }

    fn definitions(&self) -> Option<&serde_json::Map<String, Value>> {
        self.as_object()?.get("$defs")?.as_object()
    }

    fn items(&self) -> Option<&Value> {
        self.as_object()?.get("items")
    }

    fn any_of(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("anyOf")?.as_array()
    }

    fn one_of(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("oneOf")?.as_array()
    }

    fn all_of(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("allOf")?.as_array()
    }

    fn enum_values(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("enum")?.as_array()
    }

    fn ref_target(&self) -> Option<&str> {
        self.as_object()?.get("$ref")?.as_str()
    }

    fn default_value(&self) -> Option<&Value> {
        self.as_object()?.get("default")
    }

    fn required_fields(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("required")?.as_array()
    }

    fn format(&self) -> Option<&str> {
        self.as_object()?.get("format")?.as_str()
    }

    fn pattern(&self) -> Option<&str> {
        self.as_object()?.get("pattern")?.as_str()
    }

    fn minimum(&self) -> Option<f64> {
        self.as_object()?.get("minimum")?.as_f64()
    }

    fn maximum(&self) -> Option<f64> {
        self.as_object()?.get("maximum")?.as_f64()
    }

    fn const_value(&self) -> Option<&Value> {
        self.as_object()?.get("const")
    }

    fn pattern_properties(&self) -> Option<&serde_json::Map<String, Value>> {
        self.as_object()?.get("patternProperties")?.as_object()
    }

    fn additional_properties(&self) -> Option<&Value> {
        self.as_object()?.get("additionalProperties")
    }

    fn prefix_items(&self) -> Option<&Vec<Value>> {
        self.as_object()?.get("prefixItems")?.as_array()
    }

    fn min_items(&self) -> Option<u64> {
        self.as_object()?.get("minItems")?.as_u64()
    }

    fn max_items(&self) -> Option<u64> {
        self.as_object()?.get("maxItems")?.as_u64()
    }

    fn is_deprecated(&self) -> bool {
        self.as_object()
            .and_then(|o| o.get("deprecated"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_ext_title() {
        let schema: Schema =
            serde_json::from_str(r#"{"title": "Test Schema", "type": "object"}"#).unwrap();
        assert_eq!(schema.title(), Some("Test Schema"));
    }

    #[test]
    fn test_schema_ext_description() {
        let schema: Schema =
            serde_json::from_str(r#"{"description": "A test description"}"#).unwrap();
        assert_eq!(schema.description(), Some("A test description"));
    }

    #[test]
    fn test_schema_ext_ref() {
        let schema: Schema = serde_json::from_str(r##"{"$ref": "#/$defs/MyType"}"##).unwrap();
        assert_eq!(schema.ref_target(), Some("#/$defs/MyType"));
    }

    #[test]
    fn test_schema_ext_properties() {
        let schema: Schema = serde_json::from_str(
            r#"{"type": "object", "properties": {"name": {"type": "string"}}}"#,
        )
        .unwrap();
        let props = schema.properties().unwrap();
        assert!(props.contains_key("name"));
    }

    #[test]
    fn test_tuple_with_ref_items() {
        let schema: Schema = serde_json::from_str(
            r##"{
            "type": "object",
            "properties": {
                "coords": {
                    "type": "array",
                    "prefixItems": [
                        { "$ref": "#/$defs/X" },
                        { "$ref": "#/$defs/Y" }
                    ]
                }
            },
            "$defs": {
                "X": { "type": "number" },
                "Y": { "type": "number" }
            }
        }"##,
        )
        .unwrap();

        let result = generate_mdx(&schema, &MdxOptions::default());
        assert!(result.is_ok(), "Should handle $ref in prefixItems");
    }

    #[test]
    fn test_tuple_with_mixed_items() {
        let schema: Schema = serde_json::from_str(
            r##"{
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        { "$ref": "#/$defs/Name" },
                        { "type": "integer" }
                    ]
                }
            },
            "$defs": {
                "Name": { "type": "string" }
            }
        }"##,
        )
        .unwrap();

        let result = generate_mdx(&schema, &MdxOptions::default());
        assert!(
            result.is_ok(),
            "Should handle mixed $ref and type in prefixItems"
        );
    }

    #[test]
    fn test_tuple_with_anyof_items() {
        let schema: Schema = serde_json::from_str(
            r##"{
            "type": "object",
            "properties": {
                "optional_pair": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "string" },
                        { "anyOf": [{ "type": "number" }, { "type": "null" }] }
                    ]
                }
            }
        }"##,
        )
        .unwrap();

        let result = generate_mdx(&schema, &MdxOptions::default());
        assert!(result.is_ok(), "Should handle anyOf in prefixItems");
    }

    #[test]
    fn test_homogeneous_tuple_still_works() {
        let schema: Schema = serde_json::from_str(
            r#"{
            "type": "object",
            "properties": {
                "point": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "number" },
                        { "type": "number" },
                        { "type": "number" }
                    ]
                }
            }
        }"#,
        )
        .unwrap();

        let result = generate_mdx(&schema, &MdxOptions::default());
        assert!(result.is_ok());
    }
}
