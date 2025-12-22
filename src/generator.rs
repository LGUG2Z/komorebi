//! HTML generation using Tera templates

use crate::Error;
use crate::Options;
use crate::identifiers::CSS;
use crate::identifiers::IDS;
use crate::identifiers::TYPE_CLASSES;
use crate::prepare::NavigationData;
use crate::prepare::build_navigation;
use crate::prepare::embed_usages_in_defs;
use crate::prepare::prepare_for_template;
use crate::prepare::render_markdown;
use crate::prepare::strip_markdown;
use schemars::Schema;
use serde_json::Value;
use tera::Context;
use tera::Tera;

/// HTML documentation generator
pub struct Generator {
    tera: Tera,
}

impl Generator {
    /// Create a new generator with embedded templates
    pub fn new() -> Result<Self, Error> {
        let mut tera = Tera::default();

        // Add templates from embedded strings
        // Order matters: macros and style must be added before base.html which imports them
        tera.add_raw_template("style.css", include_str!("templates/style.css"))?;
        tera.add_raw_template("macros.html", include_str!("templates/macros.html"))?;
        tera.add_raw_template("base.html", include_str!("templates/base.html"))?;

        Ok(Self { tera })
    }

    /// Generate HTML documentation from a schema
    pub fn generate(&self, schema: &Schema, options: &Options) -> Result<String, Error> {
        // Prepare the schema for template rendering
        let mut prepared_schema = prepare_for_template(schema.as_value());

        // Build navigation data from the prepared schema (includes _display_name for wrapper types)
        let navigation = build_navigation(&prepared_schema);

        // Embed usages directly into $defs to avoid Tera lookup issues with special characters
        embed_usages_in_defs(&mut prepared_schema, &navigation.usages);

        // Build the template context
        let context = self.build_context(&prepared_schema, &navigation, options)?;

        // Render the template
        let html = self.tera.render("base.html", &context)?;

        Ok(html)
    }

    /// Build the Tera context from schema and options
    fn build_context(
        &self,
        schema: &Value,
        navigation: &NavigationData,
        options: &Options,
    ) -> Result<Context, Error> {
        let mut context = Context::new();

        // Title: use override, then description, then schema title
        let title = options
            .title_override
            .clone()
            .or_else(|| {
                schema
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .or_else(|| {
                schema
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| "Schema Documentation".to_string());
        // Render title as markdown HTML (for h1 display)
        let title_html = render_markdown(&title);
        // Strip markdown for plain text title (for HTML <title> tag)
        let title_plain = strip_markdown(&title);
        context.insert("title", &title_plain);
        context.insert("title_html", &title_html);

        // The full schema for property rendering
        context.insert("schema", schema);

        // Navigation data for sidebar
        context.insert("navigation", navigation);

        // Search index as JSON for the JavaScript
        context.insert("search_index", &navigation.search_index);

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

        // Add CSS classes and HTML IDs from the single source of truth
        context.insert("ids", &IDS);
        context.insert("css", &CSS);
        context.insert("type_classes", &TYPE_CLASSES);

        Ok(context)
    }
}

impl Default for Generator {
    fn default() -> Self {
        Self::new().expect("Failed to create default generator")
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
    fn test_generator_new() {
        let generator = Generator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate_simple_schema() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options);
        assert!(html.is_ok());

        let html = html.unwrap();
        // Description is now used as the page title
        assert!(html.contains("A test configuration"));
        assert!(html.contains("name"));
        assert!(html.contains("The name"));
    }

    #[test]
    fn test_generate_with_title_override() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "OriginalTitle",
            "type": "object"
        }));

        let options = Options {
            title_override: Some("Custom Title".to_string()),
        };

        let html = generator.generate(&schema, &options).unwrap();
        assert!(html.contains("Custom Title"));
        assert!(!html.contains("<title>OriginalTitle</title>"));
    }

    #[test]
    fn test_generate_with_definitions() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MyType": {
                    "type": "string",
                    "description": "A custom type"
                }
            },
            "properties": {
                "field": {
                    "$ref": "#/$defs/MyType"
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(html.contains("MyType"));
        assert!(html.contains("A custom type"));
        assert!(html.contains("href=\"#mytype\""));
    }

    #[test]
    fn test_generate_with_enum() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(html.contains("active"));
        assert!(html.contains("inactive"));
        assert!(html.contains("pending"));
    }

    #[test]
    fn test_generate_with_anyof() {
        let generator = Generator::new().unwrap();
        // Use 3 variants to ensure it's not treated as Option<T>
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "number"},
                        {"type": "boolean"}
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // anyOf now renders as "enum" badge
        assert!(html.contains(">enum<"), "Should show enum badge");
        assert!(html.contains("string"));
        assert!(html.contains("number"));
        assert!(html.contains("boolean"));
    }

    #[test]
    fn test_generate_with_markdown_description() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "description": "This is **bold** and `code`",
            "type": "object"
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Description is now used as the page title (markdown stripped for <title>)
        assert!(
            html.contains("This is bold and code"),
            "Description should be used as title with markdown stripped"
        );
        // The h1 should have rendered markdown
        assert!(
            html.contains("<strong>bold</strong>"),
            "h1 should have rendered markdown"
        );
    }

    #[test]
    fn test_generate_responsive_html() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object"
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Check for responsive viewport meta tag
        assert!(html.contains("viewport"));
        assert!(html.contains("width=device-width"));

        // Check for responsive CSS
        assert!(html.contains("@media"));
        assert!(html.contains("min-width: 768px"));
    }

    #[test]
    fn test_generate_dark_mode_css() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object"
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Check for dark mode media query
        assert!(html.contains("prefers-color-scheme: dark"));
    }

    // Regression test: macro parameter 'schema' was conflicting with global context 'schema'
    // This caused infinite recursion when rendering properties with enum values
    #[test]
    fn test_enum_property_no_infinite_recursion() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["active", "inactive", "pending"],
                    "description": "The status of the item"
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should contain enum values, not infinite nesting
        assert!(html.contains("active"));
        assert!(html.contains("inactive"));
        assert!(html.contains("pending"));

        // Should NOT have deeply nested duplicate property names (sign of infinite recursion)
        // Count occurrences of "status" in property names - should be limited
        let status_count = html.matches("data-search-name=\"status\"").count();
        assert!(
            status_count <= 2,
            "Expected at most 2 occurrences of status property, found {}",
            status_count
        );
    }

    // Regression test: anyOf properties should render correctly
    // Updated: string + null is now recognized as Option<T> pattern
    #[test]
    fn test_anyof_property_renders_variants() {
        let generator = Generator::new().unwrap();
        // Use string + number to test regular anyOf (not Option<T>)
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "value": {
                    "description": "A value that can be string or number",
                    "anyOf": [
                        {"type": "string"},
                        {"type": "number"}
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show enum badge (anyOf now renders as enum)
        assert!(html.contains(">enum<"), "Should contain enum badge");
        assert!(
            html.contains("type-string") || html.contains(">string<"),
            "Should contain string type"
        );
        assert!(
            html.contains("type-number") || html.contains(">number<"),
            "Should contain number type"
        );
    }

    // Regression test: additionalProperties check should not fail on undefined
    #[test]
    fn test_schema_without_additional_properties() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        }));

        let options = Options::default();
        // Should not error when additionalProperties is not defined
        let result = generator.generate(&schema, &options);
        assert!(result.is_ok());
    }

    // Test that descriptions are rendered as HTML (markdown processed)
    #[test]
    fn test_description_markdown_rendered() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "description": "This has **bold** and `code`",
            "type": "object",
            "properties": {
                "field": {
                    "type": "string",
                    "description": "Field with *emphasis*"
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Schema description is used as title (markdown stripped for <title>)
        assert!(
            html.contains("This has bold and code"),
            "Description should be used as title with markdown stripped"
        );
        // The h1 should have rendered markdown
        assert!(
            html.contains("<strong>bold</strong>"),
            "h1 should have rendered markdown"
        );
        // Property description markdown is still rendered to HTML
        assert!(html.contains("<em>emphasis</em>"));
    }

    // Test Option<T> (anyOf with null) is rendered as optional badge
    #[test]
    fn test_option_t_rendered_as_optional() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "optional" badge
        assert!(html.contains("optional"), "Should contain 'optional' badge");
        // Should show the string type
        assert!(
            html.contains("type-string") || html.contains(">string<"),
            "Should contain string type badge"
        );
        // Should NOT show "anyOf" badge for Option<T>
        assert!(
            !html.contains(">anyOf<"),
            "Should not show anyOf badge for Option<T>"
        );
        // Should NOT show variant markers
        assert!(
            !html.contains("Variant 1"),
            "Should not show Variant markers for Option<T>"
        );
    }

    // Test Option<T> with $ref
    #[test]
    fn test_option_t_with_ref() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MyType": { "type": "object", "description": "A custom type" }
            },
            "properties": {
                "maybe_type": {
                    "anyOf": [
                        { "$ref": "#/$defs/MyType" },
                        { "type": "null" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "optional" badge
        assert!(html.contains("optional"), "Should contain 'optional' badge");
        // Should show the type reference
        assert!(html.contains("MyType"), "Should contain MyType reference");
        // Should link to the type
        assert!(
            html.contains("href=\"#mytype\""),
            "Should link to the type definition"
        );
    }

    // Test that regular anyOf (not Option<T>) still shows all variants
    #[test]
    fn test_regular_anyof_shows_variants() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "number" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show enum badge (anyOf now renders as enum)
        assert!(html.contains(">enum<"), "Should show enum badge");
        // Should show variants summary
        assert!(
            html.contains("Enum variants (2)"),
            "Should show variants summary"
        );
    }

    // Test Option<T> with type array (e.g., type: ["boolean", "null"])
    #[test]
    fn test_option_t_with_type_array() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "enabled": {
                    "type": ["boolean", "null"],
                    "description": "Enable feature"
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "optional" badge
        assert!(html.contains("optional"), "Should contain 'optional' badge");
        // Should show boolean type
        assert!(
            html.contains("type-boolean"),
            "Should contain boolean type badge"
        );
        // Should NOT show null type badge in the content (CSS class definition is OK)
        // Check that there's no actual >null< badge text in the rendered property
        assert!(
            !html.contains(">null<"),
            "Should not show null type badge text for Option<T>"
        );
    }

    // Test outline section in sidebar
    #[test]
    fn test_outline_section_in_sidebar() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "alpha": { "type": "string" },
                "beta": { "type": "number" },
                "gamma": { "type": "boolean" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should have Outline section
        assert!(html.contains("Outline"), "Should have Outline section");
        // Should list top-level properties with links
        assert!(
            html.contains("href=\"#alpha\""),
            "Should link to alpha property"
        );
        assert!(
            html.contains("href=\"#beta\""),
            "Should link to beta property"
        );
        assert!(
            html.contains("href=\"#gamma\""),
            "Should link to gamma property"
        );
    }

    // Test HashMap with integer key pattern displays friendly type
    #[test]
    fn test_hashmap_integer_key_display() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "layout_rules": {
                    "type": "object",
                    "patternProperties": {
                        "^\\d+$": { "type": "string" }
                    }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "integer" as the key type (with quotes to indicate string)
        assert!(
            html.contains("integer"),
            "Should display 'integer' for ^\\d+$ pattern"
        );
        assert!(
            html.contains("key-type"),
            "Should have key-type class for styling"
        );
        // Should NOT show the raw regex pattern
        assert!(!html.contains("^\\d+$"), "Should not show raw pattern");
        // Should show the arrow separator
        assert!(
            html.contains("→"),
            "Should show arrow between key and value type"
        );
    }

    // Test HashMap with string key pattern displays friendly type
    #[test]
    fn test_hashmap_string_key_display() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "string_map": {
                    "type": "object",
                    "patternProperties": {
                        ".*": { "type": "number" }
                    }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "string" for .* pattern
        assert!(
            html.contains(">string<"),
            "Should display 'string' for .* pattern"
        );
        // Should NOT show the raw pattern in the output
        assert!(!html.contains(">.*<"), "Should not show raw .* pattern");
    }

    // Test HashMap with unknown pattern still shows the regex
    #[test]
    fn test_hashmap_unknown_pattern_shows_regex() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "custom_map": {
                    "type": "object",
                    "patternProperties": {
                        "^[a-z]+$": { "type": "string" }
                    }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Unknown patterns should still show the regex
        assert!(
            html.contains("^[a-z]+$"),
            "Should show unknown pattern as-is"
        );
        assert!(html.contains("pattern"), "Should have pattern badge");
    }

    // Test: Array of primitives should not show "Array Items" dropdown
    #[test]
    fn test_array_of_primitives_no_dropdown() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "strings": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show array and string badges
        assert!(html.contains("type-array"), "Should have array badge");
        assert!(html.contains("type-string"), "Should have string badge");
        // Should NOT show Array Items dropdown
        assert!(
            !html.contains("Array Items"),
            "Should not show redundant Array Items dropdown for simple types"
        );
    }

    // Test: Homogeneous tuple should use compact [T; N] notation
    #[test]
    fn test_homogeneous_tuple_compact_notation() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "point": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "integer" },
                        { "type": "integer" }
                    ],
                    "minItems": 2,
                    "maxItems": 2
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show [array] badge followed by compact notation [integer; 2]
        assert!(html.contains("type-array"), "Should have array badge");
        assert!(
            html.contains("integer; 2"),
            "Should show compact tuple notation [integer; 2]"
        );
        // Should NOT show Tuple Items dropdown
        assert!(
            !html.contains("Tuple Items"),
            "Should not show Tuple Items dropdown for homogeneous tuples"
        );
        // Should NOT show min/max items constraints
        assert!(
            !html.contains("Min items"),
            "Should not show Min items constraint for tuples"
        );
        assert!(
            !html.contains("Max items"),
            "Should not show Max items constraint for tuples"
        );
    }

    // Test: Heterogeneous tuple should show dropdown
    #[test]
    fn test_heterogeneous_tuple_shows_dropdown() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "mixed": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "string" },
                        { "type": "integer" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show tuple badge (heterogeneous)
        assert!(html.contains(">tuple<"), "Should show tuple badge");
        // Should show Tuple Items dropdown
        assert!(
            html.contains("Tuple Items"),
            "Should show Tuple Items dropdown for heterogeneous tuples"
        );
        // Should show index markers
        assert!(html.contains("[0]"), "Should show [0] index");
        assert!(html.contains("[1]"), "Should show [1] index");
    }

    // Test: String enum with descriptions uses variant divs
    #[test]
    fn test_string_enum_uses_variant_divs() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "mode": {
                    "oneOf": [
                        { "type": "string", "const": "A", "description": "Option A" },
                        { "type": "string", "const": "B", "description": "Option B" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should use variant divs
        assert!(
            html.contains("class=\"variant\""),
            "Should use variant divs for string enum"
        );
        // Should NOT use ul/li enum-values
        assert!(
            !html.contains("class=\"enum-values\""),
            "Should not use ul.enum-values for string enum with descriptions"
        );
    }

    // Test: Complex enum (object variants) shows enum badge, not oneOf
    #[test]
    fn test_complex_enum_shows_enum_badge() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "config": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": { "mode": { "type": "string" } }
                        },
                        {
                            "type": "object",
                            "properties": { "level": { "type": "number" } }
                        }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show enum badge
        assert!(html.contains(">enum<"), "Should show enum badge");
        // Should NOT show oneOf badge
        assert!(!html.contains(">oneOf<"), "Should not show oneOf badge");
        // Should show "Enum variants" header
        assert!(
            html.contains("Enum variants"),
            "Should show 'Enum variants' header"
        );
        // Should NOT show "One of" header
        assert!(!html.contains("One of"), "Should not show 'One of' header");
    }

    // Priority 1: Constraint rendering tests

    #[test]
    fn test_format_constraint_renders() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("Format:") && html.contains("email"),
            "Should render format constraint"
        );
    }

    #[test]
    fn test_pattern_constraint_renders() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("Pattern:") && html.contains("^[A-Z]{3}$"),
            "Should render pattern constraint"
        );
    }

    #[test]
    fn test_min_max_constraints_render() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(html.contains("Min:"), "Should render minimum constraint");
        assert!(html.contains("Max:"), "Should render maximum constraint");
    }

    // Priority 1: allOf rendering tests

    #[test]
    fn test_allof_renders_header() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("All of (2 schemas)"),
            "Should render 'All of' header with count"
        );
    }

    // Priority 1: const rendering tests

    #[test]
    fn test_const_badge_renders() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "version": {
                    "const": "1.0.0"
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("type-const"),
            "Should render const type badge"
        );
    }

    #[test]
    fn test_const_value_displays() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "api_version": {
                    "type": "string",
                    "const": "v2"
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Value is HTML-encoded as &quot;v2&quot;
        assert!(
            html.contains("const-value") && html.contains("v2"),
            "Should display const value"
        );
    }

    // Priority 1: deprecated badge rendering tests

    #[test]
    fn test_deprecated_badge_renders() {
        let generator = Generator::new().unwrap();
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("class=\"badge deprecated\""),
            "Should render deprecated badge"
        );
        assert!(
            html.contains(">deprecated<"),
            "Should show 'deprecated' text"
        );
    }

    // Priority 1: additionalProperties rendering tests

    #[test]
    fn test_additional_properties_section_renders() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "metadata": {
                    "type": "object",
                    "additionalProperties": {
                        "type": "string"
                    }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("Additional Properties"),
            "Should render Additional Properties section"
        );
    }

    // Priority 2: Edge case rendering tests

    #[test]
    fn test_empty_properties_renders() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {}
        }));

        let options = Options::default();
        let result = generator.generate(&schema, &options);

        assert!(result.is_ok(), "Should not crash on empty properties");
    }

    #[test]
    fn test_schema_without_title_uses_fallback() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(
            html.contains("Schema Documentation"),
            "Should use fallback title"
        );
    }

    #[test]
    fn test_empty_definitions_renders() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {}
        }));

        let options = Options::default();
        let result = generator.generate(&schema, &options);

        assert!(result.is_ok(), "Should not crash on empty definitions");
    }

    #[test]
    fn test_multiple_pattern_properties_all_render() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "config": {
                    "type": "object",
                    "patternProperties": {
                        "^[a-z]+$": { "type": "string" },
                        "^[A-Z_]+$": { "type": "number" }
                    }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Both patterns should be rendered (unknown patterns show as regex)
        assert!(html.contains("^[a-z]+$"), "Should render first pattern");
        assert!(html.contains("^[A-Z_]+$"), "Should render second pattern");
    }

    // Priority 3: Type badge branch coverage

    #[test]
    fn test_array_with_object_items_shows_both_badges() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" }
                        }
                    }
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        assert!(html.contains("type-array"), "Should show array badge");
        assert!(html.contains("type-object"), "Should show object badge");
    }

    #[test]
    fn test_minlength_zero_renders() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "minLength": 0,
                    "maxLength": 10
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // minLength: 0 should still render (not be falsely skipped)
        assert!(
            html.contains("Min length:") && html.contains("0"),
            "Should render minLength: 0"
        );
        assert!(html.contains("Max length:"), "Should render maxLength");
    }

    // Test: Homogeneous tuple with floats should display "float" instead of "number"
    #[test]
    fn test_tuple_float_display() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "point": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "number" },
                        { "type": "number" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "float" instead of "number" for tuple display
        assert!(
            html.contains("float; 2"),
            "Should display 'float; 2' for number tuples"
        );
        assert!(
            !html.contains("number; 2"),
            "Should NOT display 'number; 2' for float tuples"
        );
        assert!(
            html.contains("type-float"),
            "Should use type-float CSS class"
        );
    }

    // Test: Homogeneous tuple with integers should still display "integer"
    #[test]
    fn test_tuple_integer_display_unchanged() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "coords": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "integer" },
                        { "type": "integer" },
                        { "type": "integer" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should still show "integer" for integer tuples
        assert!(
            html.contains("integer; 3"),
            "Should display 'integer; 3' for integer tuples"
        );
        assert!(
            html.contains("type-integer"),
            "Should use type-integer CSS class"
        );
    }

    // Test: Float tuple with format (e.g., format: "double") still displays "float"
    #[test]
    fn test_tuple_float_with_format() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "bezier": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "number", "format": "double" },
                        { "type": "number", "format": "double" },
                        { "type": "number", "format": "double" },
                        { "type": "number", "format": "double" }
                    ]
                }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show "float" for number tuples regardless of format
        assert!(
            html.contains("float; 4"),
            "Should display 'float; 4' for number tuples with format"
        );
    }

    // ==================== Wrapper Type Rendering Tests ====================

    // Test: Wrapper type with $ref renders inner type as link
    #[test]
    fn test_wrapper_type_renders_inner_ref() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "InnerType": {
                    "type": "string",
                    "description": "The inner type"
                },
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

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // WrapperType definition should show InnerType as a link
        assert!(
            html.contains("href=\"#innertype\""),
            "Should link to inner type definition"
        );
        // Should NOT show "enum" badge for wrapper type
        // (Note: The wrapper type is referenced via $ref, so we check the definition section)
    }

    // Test: Wrapper type with primitive renders primitive badge
    #[test]
    fn test_wrapper_type_renders_primitive_badge() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "BoolWrapper": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "type": "boolean" }
                        },
                        { "type": "boolean" }
                    ]
                }
            },
            "properties": {
                "enabled": { "$ref": "#/$defs/BoolWrapper" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show boolean type badge in the BoolWrapper definition
        assert!(
            html.contains("type-boolean"),
            "Should show boolean type badge for wrapper with primitive inner type"
        );
    }

    // Test: Wrapper type with integer constraints renders integer badge
    #[test]
    fn test_wrapper_type_with_integer_renders_correctly() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "IntWrapper": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": {
                                "type": "integer",
                                "format": "uint64",
                                "minimum": 0
                            }
                        },
                        {
                            "type": "integer",
                            "format": "uint64",
                            "minimum": 0
                        }
                    ]
                }
            },
            "properties": {
                "duration": { "$ref": "#/$defs/IntWrapper" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show integer type badge
        assert!(
            html.contains("type-integer"),
            "Should show integer type badge for wrapper with integer inner type"
        );
    }

    // Test: Non-wrapper anyOf still shows enum badge
    #[test]
    fn test_non_wrapper_anyof_still_shows_enum() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "NotWrapper": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "type": "string" }
                        },
                        { "type": "integer" }
                    ]
                }
            },
            "properties": {
                "value": { "$ref": "#/$defs/NotWrapper" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should show enum badge since inner types don't match
        assert!(
            html.contains(">enum<"),
            "Should show enum badge for non-wrapper anyOf"
        );
    }

    // Test: Wrapper type does not show "Enum variants" dropdown
    #[test]
    fn test_wrapper_type_no_enum_variants_dropdown() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "StringWrapper": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "type": "string" }
                        },
                        { "type": "string" }
                    ]
                }
            },
            "properties": {
                "name": { "$ref": "#/$defs/StringWrapper" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Count occurrences of "Enum variants" - should not appear for wrapper types
        // (The StringWrapper definition should not have an "Enum variants" dropdown)
        let enum_variants_in_stringwrapper = html
            .find("StringWrapper")
            .map(|pos| {
                let section = &html[pos..pos.min(html.len()).saturating_add(500)];
                section.contains("Enum variants")
            })
            .unwrap_or(false);

        assert!(
            !enum_variants_in_stringwrapper,
            "Wrapper type should NOT show 'Enum variants' dropdown"
        );
    }

    // Test: Wrapper type with $ref to AnimationStyle (like PerAnimationPrefixConfig3)
    #[test]
    fn test_wrapper_type_like_per_animation_prefix_config() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "AnimationStyle": {
                    "type": "string",
                    "enum": ["Linear", "EaseIn", "EaseOut"]
                },
                "PerAnimationPrefixConfig": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "$ref": "#/$defs/AnimationStyle" }
                        },
                        { "$ref": "#/$defs/AnimationStyle" }
                    ]
                }
            },
            "properties": {
                "style": { "$ref": "#/$defs/PerAnimationPrefixConfig" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // The PerAnimationPrefixConfig definition should link to AnimationStyle
        assert!(
            html.contains("href=\"#animationstyle\""),
            "Wrapper type should link to inner type (AnimationStyle)"
        );
    }

    // Test: Wrapper types display base name without numeric suffix
    #[test]
    fn test_wrapper_type_display_name() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "PerAnimationPrefixConfig": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "type": "boolean" }
                        },
                        { "type": "boolean" }
                    ]
                },
                "PerAnimationPrefixConfig2": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "type": "integer" }
                        },
                        { "type": "integer" }
                    ]
                },
                "PerAnimationPrefixConfig3": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "$ref": "#/$defs/AnimationStyle" }
                        },
                        { "$ref": "#/$defs/AnimationStyle" }
                    ]
                },
                "AnimationStyle": {
                    "type": "string",
                    "enum": ["Linear", "EaseIn"]
                }
            },
            "properties": {
                "config1": { "$ref": "#/$defs/PerAnimationPrefixConfig" },
                "config2": { "$ref": "#/$defs/PerAnimationPrefixConfig2" },
                "config3": { "$ref": "#/$defs/PerAnimationPrefixConfig3" }
            }
        }));

        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Definition headers should show base name without numeric suffix
        // PerAnimationPrefixConfig2 header should show "PerAnimationPrefixConfig"

        // Count definition-name headers containing "PerAnimationPrefixConfig" (base name)
        // The HTML has whitespace/newlines in formatting
        let header_pattern = "<h3 class=\"definition-name\">";
        let per_anim_header_count = html
            .match_indices(header_pattern)
            .filter(|(i, _)| {
                let after = &html[*i + header_pattern.len()..];
                after.trim_start().starts_with("PerAnimationPrefixConfig")
            })
            .count();

        assert_eq!(
            per_anim_header_count, 3,
            "All 3 wrapper types should show 'PerAnimationPrefixConfig' in header (found {})",
            per_anim_header_count
        );

        // Verify that PerAnimationPrefixConfig2 and PerAnimationPrefixConfig3 do NOT appear in headers
        // (They should only appear in anchor IDs and href links)
        assert!(
            !html.contains("class=\"definition-name\">PerAnimationPrefixConfig2") &&
            !html.contains("class=\"definition-name\">\n                            PerAnimationPrefixConfig2"),
            "Should NOT show numeric suffix 2 in definition header"
        );
        assert!(
            !html.contains("class=\"definition-name\">PerAnimationPrefixConfig3") &&
            !html.contains("class=\"definition-name\">\n                            PerAnimationPrefixConfig3"),
            "Should NOT show numeric suffix 3 in definition header"
        );

        // Sidebar should also show base names (navigation uses display_name)
        let sidebar_base_name_count = html.matches(">PerAnimationPrefixConfig<").count();
        assert!(
            sidebar_base_name_count >= 3,
            "Sidebar should show base name for wrapper types (found {})",
            sidebar_base_name_count
        );

        // Anchors should still use full names for correct linking
        assert!(
            html.contains("id=\"peranimationprefixconfig\""),
            "Anchor should use full name"
        );
        assert!(
            html.contains("id=\"peranimationprefixconfig2\""),
            "Anchor should use full name with suffix"
        );
        assert!(
            html.contains("id=\"peranimationprefixconfig3\""),
            "Anchor should use full name with suffix"
        );
    }

    // Test: No broken anchor links in rendered output
    #[test]
    fn test_no_broken_anchor_links() {
        use regex::Regex;
        use std::collections::HashSet;

        let generator = Generator::new().unwrap();
        let schema_json = include_str!("../schema.json");
        let schema: Schema = serde_json::from_str(schema_json).unwrap();
        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Extract all href="#..." targets (excluding JavaScript template literals like ${...})
        let href_re = Regex::new(r##"href="#([^"]+)""##).unwrap();
        let hrefs: HashSet<String> = href_re
            .captures_iter(&html)
            .map(|c| c[1].to_string())
            .filter(|href| !href.starts_with("${")) // Exclude JS template literals
            .collect();

        // Extract all id="..." values
        let id_re = Regex::new(r##"id="([^"]+)""##).unwrap();
        let ids: HashSet<String> = id_re
            .captures_iter(&html)
            .map(|c| c[1].to_string())
            .collect();

        // Find broken links
        let broken: Vec<&String> = hrefs.iter().filter(|href| !ids.contains(*href)).collect();

        if !broken.is_empty() {
            eprintln!("Broken anchor links found: {:?}", broken);
        }

        assert!(
            broken.is_empty(),
            "Found {} broken anchor links: {:?}",
            broken.len(),
            broken.iter().take(20).collect::<Vec<_>>()
        );
    }

    // Test: Modal has navigation buttons
    #[test]
    fn test_modal_has_navigation_buttons() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "field": { "type": "string" }
            }
        }));
        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should have modal navigation container
        assert!(
            html.contains("modal-nav"),
            "Should have modal-nav container"
        );
        // Should have back and forward buttons
        assert!(
            html.contains("data-dir=\"back\""),
            "Should have back button"
        );
        assert!(
            html.contains("data-dir=\"forward\""),
            "Should have forward button"
        );
        // Should have navigation button class
        assert!(
            html.contains("modal-nav-btn"),
            "Should have nav button class"
        );
    }

    // Test: Modal nav buttons start disabled
    #[test]
    fn test_modal_nav_buttons_start_disabled() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "field": { "type": "string" }
            }
        }));
        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Both nav buttons should start with disabled class
        assert!(
            html.contains("modal-nav-btn disabled"),
            "Nav buttons should start disabled"
        );
    }

    // Test: Modal has keyboard navigation
    #[test]
    fn test_modal_has_keyboard_navigation() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "field": { "type": "string" }
            }
        }));
        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should have Alt+Arrow keyboard handling
        assert!(html.contains("altKey"), "Should handle Alt key modifier");
        assert!(
            html.contains("ArrowLeft"),
            "Should handle ArrowLeft for back"
        );
        assert!(
            html.contains("ArrowRight"),
            "Should handle ArrowRight for forward"
        );
    }

    // Test: Modal has history management
    #[test]
    fn test_modal_has_history_management() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "field": { "type": "string" }
            }
        }));
        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // Should have history array and index
        assert!(
            html.contains("history=[]"),
            "Should initialize history array"
        );
        assert!(
            html.contains("historyIndex=-1"),
            "Should initialize history index"
        );
        // Should have navigate function
        assert!(
            html.contains("navigateHistory"),
            "Should have navigateHistory function"
        );
    }

    // Test: Modal history cleared on close
    #[test]
    fn test_modal_history_cleared_on_close() {
        let generator = Generator::new().unwrap();
        let schema = make_schema(json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "field": { "type": "string" }
            }
        }));
        let options = Options::default();
        let html = generator.generate(&schema, &options).unwrap();

        // closeModal function should reset history
        // The function contains: history=[];historyIndex=-1;
        assert!(
            html.contains("history=[]") && html.contains("historyIndex=-1"),
            "closeModal should reset history state"
        );
    }
}
