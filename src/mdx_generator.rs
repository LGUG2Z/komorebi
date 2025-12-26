//! MDX generation using Tera templates for Astro Starlight

use crate::Error;
use crate::MdxOptions;
use crate::prepare::NavigationData;
use crate::prepare::build_navigation;
use crate::prepare::embed_usages_in_defs;
use crate::prepare::prepare_for_template;
use crate::prepare::strip_markdown;
use schemars::Schema;
use serde_json::Value;
use std::collections::HashMap;
use tera::Context;
use tera::Tera;

/// Tera filter to escape curly braces for MDX
///
/// MDX interprets `{` and `}` as JSX expressions. This filter escapes them
/// so they render as literal text, which is necessary for descriptions
/// containing JSON code blocks.
///
/// This filter is code-block aware: it will NOT escape braces inside fenced
/// code blocks (```) or inline code (`), preserving proper code formatting.
fn mdx_escape(value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
    match value {
        Value::String(s) => {
            let escaped = escape_braces_outside_code(s);
            Ok(Value::String(escaped))
        }
        _ => Ok(value.clone()),
    }
}

/// Escape `{` and `}` characters outside of code blocks and inline code.
///
/// This preserves code block content while escaping braces in regular text
/// that would otherwise be interpreted as JSX expressions by MDX.
fn escape_braces_outside_code(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    // Track whether we're inside a fenced code block
    let mut in_fenced_block = false;

    while i < len {
        // Check for fenced code block start/end (```)
        if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            // Toggle fenced block state
            in_fenced_block = !in_fenced_block;
            result.push('`');
            result.push('`');
            result.push('`');
            i += 3;
            continue;
        }

        // If we're in a fenced code block, pass through everything
        if in_fenced_block {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Check for inline code (single backtick, not part of ```)
        if chars[i] == '`' {
            result.push('`');
            i += 1;
            // Find closing backtick and pass through content
            while i < len && chars[i] != '`' {
                result.push(chars[i]);
                i += 1;
            }
            // Push closing backtick if found
            if i < len {
                result.push('`');
                i += 1;
            }
            continue;
        }

        // Outside of any code: escape braces
        if chars[i] == '{' {
            result.push_str("\\{");
        } else if chars[i] == '}' {
            result.push_str("\\}");
        } else {
            result.push(chars[i]);
        }
        i += 1;
    }

    result
}

/// MDX documentation generator for Astro Starlight
pub struct MdxGenerator {
    tera: Tera,
}

impl MdxGenerator {
    /// Create a new MDX generator with embedded templates
    pub fn new() -> Result<Self, Error> {
        let mut tera = Tera::default();

        // Register custom filters
        tera.register_filter("mdx_escape", mdx_escape);

        // Add MDX-specific templates
        // Order matters: macros must be added before base which imports them
        tera.add_raw_template("mdx_macros.md", include_str!("templates/mdx_macros.md"))?;
        tera.add_raw_template("mdx_base.md", include_str!("templates/mdx_base.md"))?;

        Ok(Self { tera })
    }

    /// Generate MDX documentation from a schema
    pub fn generate(&self, schema: &Schema, options: &MdxOptions) -> Result<String, Error> {
        // Prepare the schema for template rendering (reuse existing preparation logic)
        let mut prepared_schema = prepare_for_template(schema.as_value());

        // Build navigation data from the prepared schema
        let navigation = build_navigation(&prepared_schema);

        // Embed usages directly into $defs
        embed_usages_in_defs(&mut prepared_schema, &navigation.usages);

        // Build the template context
        let context = self.build_context(&prepared_schema, &navigation, options)?;

        // Render the MDX template
        let mdx = self.tera.render("mdx_base.md", &context)?;

        Ok(mdx)
    }

    /// Build the Tera context from schema and options
    fn build_context(
        &self,
        schema: &Value,
        navigation: &NavigationData,
        options: &MdxOptions,
    ) -> Result<Context, Error> {
        let mut context = Context::new();

        // Title: use override, then schema title, then fallback
        let title = options
            .title_override
            .clone()
            .or_else(|| {
                schema
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| "Schema Documentation".to_string());
        // Strip markdown for plain text title (for frontmatter)
        let title_plain = strip_markdown(&title);
        context.insert("title", &title_plain);

        // Description: use override, then schema description, then title
        let description = options
            .description_override
            .clone()
            .or_else(|| {
                schema
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .or_else(|| Some(title_plain.clone()))
            .unwrap_or_default();
        // Strip markdown and sanitize for YAML frontmatter
        let description_plain = strip_markdown(&description)
            .replace(['\n', '\r'], " ")
            .replace('"', "'"); // Replace double quotes with single quotes for YAML safety
        // Truncate very long descriptions for frontmatter
        let description_truncated = if description_plain.len() > 200 {
            format!("{}...", &description_plain[..197])
        } else {
            description_plain
        };
        context.insert("description", &description_truncated);

        // The full schema for property rendering
        context.insert("schema", schema);

        // Navigation data
        context.insert("navigation", navigation);

        // Type usages for "Usages" section in type definitions
        context.insert("usages", &navigation.usages);

        // Display names for wrapper types (type_name -> display_name)
        context.insert("display_names", &navigation.display_names);

        // Build required set for the root schema
        let required_set: Vec<String> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        context.insert("required_set", &required_set);

        Ok(context)
    }
}

impl Default for MdxGenerator {
    fn default() -> Self {
        Self::new().expect("Failed to create default MDX generator")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_schema(value: Value) -> Schema {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn test_mdx_generator_new() {
        let generator = MdxGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate_simple_schema() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "TestConfig",
            "description": "A test configuration",
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The name"
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options);
        assert!(mdx.is_ok());

        let mdx = mdx.unwrap();
        // Check frontmatter
        assert!(mdx.contains("---"), "Should have frontmatter delimiters");
        assert!(
            mdx.contains("title: \"TestConfig\""),
            "Should have title in frontmatter"
        );
        // Check content
        assert!(mdx.contains("name"), "Should contain property name");
        assert!(
            mdx.contains("The name"),
            "Should contain property description"
        );
    }

    #[test]
    fn test_generate_with_title_override() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "OriginalTitle",
            "type": "object"
        }));

        let options = MdxOptions {
            title_override: Some("Custom Title".to_string()),
            ..Default::default()
        };

        let mdx = generator.generate(&schema, &options).unwrap();
        assert!(mdx.contains("title: \"Custom Title\""));
        assert!(!mdx.contains("title: \"OriginalTitle\""));
    }

    #[test]
    fn test_generate_with_description_override() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "description": "Original description",
            "type": "object"
        }));

        let options = MdxOptions {
            description_override: Some("Custom description".to_string()),
            ..Default::default()
        };

        let mdx = generator.generate(&schema, &options).unwrap();
        assert!(mdx.contains("description: \"Custom description\""));
    }

    #[test]
    fn test_frontmatter_generation() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "My Schema",
            "description": "Schema description",
            "type": "object"
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Frontmatter should be at the start
        assert!(mdx.starts_with("---"), "MDX should start with frontmatter");
        // Should have closing delimiter
        let parts: Vec<&str> = mdx.splitn(3, "---").collect();
        assert!(
            parts.len() >= 3,
            "Should have opening and closing frontmatter delimiters"
        );
    }

    #[test]
    fn test_component_imports() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "field": { "type": "string" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should import Badge component from Starlight
        assert!(
            mdx.contains("import { Badge }") || mdx.contains("import {Badge}"),
            "Should import Badge component"
        );
        assert!(
            mdx.contains("@astrojs/starlight/components"),
            "Should import from Starlight"
        );
    }

    #[test]
    fn test_heading_structure() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "alpha": { "type": "string" },
                "beta": { "type": "number" }
            },
            "$defs": {
                "MyType": { "type": "string" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should have H2 for main sections
        assert!(
            mdx.contains("## Configuration"),
            "Should have Configuration H2"
        );
        assert!(
            mdx.contains("## Type Definitions"),
            "Should have Type Definitions H2"
        );

        // Should have H3 for properties and types (using markdown headings with custom IDs)
        assert!(mdx.contains("### alpha {#alpha}"), "Should have alpha H3");
        assert!(mdx.contains("### beta {#beta}"), "Should have beta H3");
        assert!(
            mdx.contains("### MyType {#mytype}"),
            "Should have MyType H3"
        );
    }

    #[test]
    fn test_primitive_type_badges() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "str_field": { "type": "string" },
                "num_field": { "type": "number" },
                "int_field": { "type": "integer" },
                "bool_field": { "type": "boolean" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Primitive types should be shown as inline code
        assert!(mdx.contains("`string`"), "Should show string type");
        assert!(mdx.contains("`number`"), "Should show number type");
        assert!(mdx.contains("`integer`"), "Should show integer type");
        assert!(mdx.contains("`boolean`"), "Should show boolean type");
    }

    #[test]
    fn test_required_property_badge() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Required properties should have a Badge
        assert!(
            mdx.contains("Badge") && mdx.contains("required"),
            "Should have required badge"
        );
    }

    #[test]
    fn test_deprecated_property_badge() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "old_field": {
                    "type": "string",
                    "deprecated": true
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Deprecated properties should have a Badge
        assert!(
            mdx.contains("Badge") && mdx.contains("deprecated"),
            "Should have deprecated badge"
        );
    }

    #[test]
    fn test_optional_property_badge() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "maybe_name": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "null" }
                    ]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Option<T> pattern should show optional badge
        assert!(
            mdx.contains("optional"),
            "Should have optional badge for Option<T>"
        );
    }

    #[test]
    fn test_ref_type_links() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MyType": { "type": "string" }
            },
            "properties": {
                "field": { "$ref": "#/$defs/MyType" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should have a link to the type definition
        assert!(
            mdx.contains("[MyType](#mytype)") || mdx.contains("[`MyType`](#mytype)"),
            "Should have link to type definition"
        );
    }

    #[test]
    fn test_enum_values() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["active", "inactive", "pending"]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should list enum values
        assert!(mdx.contains("active"), "Should contain enum value 'active'");
        assert!(
            mdx.contains("inactive"),
            "Should contain enum value 'inactive'"
        );
        assert!(
            mdx.contains("pending"),
            "Should contain enum value 'pending'"
        );
    }

    #[test]
    fn test_oneof_variants() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "choice": {
                    "oneOf": [
                        { "type": "string", "const": "A", "description": "Option A" },
                        { "type": "string", "const": "B", "description": "Option B" }
                    ]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show variants
        assert!(
            mdx.contains("Option A") || mdx.contains("\"A\""),
            "Should contain variant A"
        );
        assert!(
            mdx.contains("Option B") || mdx.contains("\"B\""),
            "Should contain variant B"
        );
    }

    #[test]
    fn test_anyof_variants() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "number" },
                        { "type": "boolean" }
                    ]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show all variant types
        assert!(mdx.contains("string"), "Should contain string variant");
        assert!(mdx.contains("number"), "Should contain number variant");
        assert!(mdx.contains("boolean"), "Should contain boolean variant");
    }

    #[test]
    fn test_array_type_with_items() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show array type
        assert!(mdx.contains("array"), "Should contain array type");
        assert!(mdx.contains("string"), "Should contain item type");
    }

    #[test]
    fn test_format_constraint() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "email": {
                    "type": "string",
                    "format": "email"
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show format constraint
        assert!(mdx.contains("email"), "Should show format constraint");
    }

    #[test]
    fn test_pattern_constraint() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "pattern": "^[A-Z]{3}$"
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show pattern constraint
        assert!(mdx.contains("^[A-Z]{3}$"), "Should show pattern constraint");
    }

    #[test]
    fn test_min_max_constraints() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "age": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 120
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show min/max constraints
        assert!(mdx.contains("0"), "Should show minimum");
        assert!(mdx.contains("120"), "Should show maximum");
    }

    #[test]
    fn test_usage_links() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "SharedType": { "type": "string", "description": "A shared type" }
            },
            "properties": {
                "field1": { "$ref": "#/$defs/SharedType" },
                "field2": { "$ref": "#/$defs/SharedType" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Type definition should have usage links
        assert!(
            mdx.contains("field1") && mdx.contains("field2"),
            "Should show where the type is used"
        );
    }

    #[test]
    fn test_anchor_link_format() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MyType": { "type": "string" }
            },
            "properties": {
                "field": { "$ref": "#/$defs/MyType" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Anchor links should be lowercase
        assert!(mdx.contains("#mytype"), "Anchor links should be lowercase");
    }

    #[test]
    fn test_wrapper_types() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "InnerType": { "type": "string" },
                "WrapperType": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "$ref": "#/$defs/InnerType" }
                        },
                        { "$ref": "#/$defs/InnerType" }
                    ]
                }
            },
            "properties": {
                "config": { "$ref": "#/$defs/WrapperType" }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Wrapper types should show inner type
        assert!(
            mdx.contains("InnerType") || mdx.contains("innertype"),
            "Should show inner type for wrapper"
        );
    }

    #[test]
    fn test_option_t_pattern() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "optional_value": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "null" }
                    ]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show optional badge, not enum variants
        assert!(mdx.contains("optional"), "Should show optional badge");
        // Should show the actual type
        assert!(mdx.contains("string"), "Should show string type");
    }

    #[test]
    fn test_allof_schemas() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "combined": {
                    "allOf": [
                        { "type": "object", "properties": { "name": { "type": "string" } } },
                        { "type": "object", "properties": { "age": { "type": "integer" } } }
                    ]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show allOf content
        assert!(
            mdx.contains("name"),
            "Should show first allOf schema properties"
        );
        assert!(
            mdx.contains("age"),
            "Should show second allOf schema properties"
        );
    }

    #[test]
    fn test_homogeneous_tuple() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "point": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "number" },
                        { "type": "number" }
                    ],
                    "minItems": 2,
                    "maxItems": 2
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // Should show compact tuple notation
        assert!(
            mdx.contains("float; 2")
                || mdx.contains("number; 2")
                || mdx.contains("[float; 2]")
                || mdx.contains("[number; 2]"),
            "Should show compact tuple notation"
        );
    }

    #[test]
    fn test_string_enum_detection() {
        let generator = MdxGenerator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "mode": {
                    "oneOf": [
                        { "type": "string", "const": "A", "description": "Mode A" },
                        { "type": "string", "const": "B", "description": "Mode B" }
                    ]
                }
            }
        }));

        let options = MdxOptions::default();
        let mdx = generator.generate(&schema, &options).unwrap();

        // String enum should be detected
        assert!(
            mdx.contains("enum") || mdx.contains("Enum"),
            "Should detect string enum pattern"
        );
    }

    #[test]
    fn test_escape_braces_outside_code_plain_text() {
        // Plain text should have braces escaped
        let result = escape_braces_outside_code("Use {foo} and {bar}");
        assert_eq!(result, "Use \\{foo\\} and \\{bar\\}");
    }

    #[test]
    fn test_escape_braces_outside_code_inline_code() {
        // Braces inside inline code should NOT be escaped
        let result = escape_braces_outside_code("Use `{foo}` in your code");
        assert_eq!(result, "Use `{foo}` in your code");
    }

    #[test]
    fn test_escape_braces_outside_code_fenced_block() {
        // Braces inside fenced code blocks should NOT be escaped
        let input = "Example:\n```json\n{\"key\": \"value\"}\n```\nEnd";
        let result = escape_braces_outside_code(input);
        assert_eq!(result, "Example:\n```json\n{\"key\": \"value\"}\n```\nEnd");
    }

    #[test]
    fn test_escape_braces_outside_code_mixed() {
        // Mix of plain text and code - only plain text braces escaped
        let input = "Set `{option}` to {value}";
        let result = escape_braces_outside_code(input);
        assert_eq!(result, "Set `{option}` to \\{value\\}");
    }

    #[test]
    fn test_escape_braces_outside_code_multiple_code_blocks() {
        // Multiple fenced code blocks
        let input = "First:\n```\n{a}\n```\nMiddle {b}\n```\n{c}\n```\nEnd";
        let result = escape_braces_outside_code(input);
        assert_eq!(
            result,
            "First:\n```\n{a}\n```\nMiddle \\{b\\}\n```\n{c}\n```\nEnd"
        );
    }

    #[test]
    fn test_escape_braces_outside_code_no_braces() {
        // No braces should pass through unchanged
        let result = escape_braces_outside_code("Just plain text");
        assert_eq!(result, "Just plain text");
    }

    #[test]
    fn test_escape_braces_outside_code_nested_inline() {
        // Multiple inline code segments
        let result = escape_braces_outside_code("`{a}` and `{b}` with {c}");
        assert_eq!(result, "`{a}` and `{b}` with \\{c\\}");
    }
}
