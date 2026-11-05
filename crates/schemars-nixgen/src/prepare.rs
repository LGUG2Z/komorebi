//! Schema preparation utilities for normalizing JSON Schema before conversion

use serde_json::Value;
use std::collections::BTreeMap;

/// Result type for mixed enum + object union detection
/// Contains (enum_variants, object_variants) where object_variants are (tag_name, inner_schema) tuples
pub type MixedUnionResult = (Vec<String>, Vec<(String, Value)>);

/// Extract all definitions from the schema's $defs section
pub fn collect_definitions(schema: &Value) -> BTreeMap<String, Value> {
    let mut defs = BTreeMap::new();

    if let Some(definitions) = schema.get("$defs").and_then(|d| d.as_object()) {
        for (name, def) in definitions {
            defs.insert(name.clone(), def.clone());
        }
    }

    // Also check "definitions" for older schemas
    if let Some(definitions) = schema.get("definitions").and_then(|d| d.as_object()) {
        for (name, def) in definitions {
            defs.insert(name.clone(), def.clone());
        }
    }

    defs
}

/// Resolve a $ref to its definition name
pub fn resolve_ref_name(ref_str: &str) -> Option<&str> {
    // Handle "#/$defs/TypeName" or "#/definitions/TypeName"
    if let Some(name) = ref_str.strip_prefix("#/$defs/") {
        Some(name)
    } else if let Some(name) = ref_str.strip_prefix("#/definitions/") {
        Some(name)
    } else {
        None
    }
}

/// Check if a schema represents a nullable type (anyOf with null)
/// Returns Some(inner_schema) if it's a nullable pattern, None otherwise
/// Handles cases with multiple null variants (e.g., from serde Option serialization)
pub fn detect_nullable_anyof(schema: &Value) -> Option<&Value> {
    let any_of = schema.get("anyOf")?.as_array()?;

    // Collect null and non-null variants
    let mut non_null_variants: Vec<&Value> = Vec::new();
    let mut has_null = false;

    for variant in any_of.iter() {
        if is_null_schema(variant) {
            has_null = true;
        } else {
            non_null_variants.push(variant);
        }
    }

    // If we have exactly one non-null variant and at least one null, it's a nullable pattern
    if has_null && non_null_variants.len() == 1 {
        Some(non_null_variants[0])
    } else {
        None
    }
}

/// Check if a schema is a null type
pub fn is_null_schema(schema: &Value) -> bool {
    // Check for {"type": "null"}
    schema
        .get("type")
        .is_some_and(|type_val| type_val == "null")
}

/// Check if a type array includes null: ["string", "null"]
/// Returns Some(non_null_type) if it's a nullable type array
pub fn detect_nullable_type_array(schema: &Value) -> Option<String> {
    let type_val = schema.get("type")?;
    let type_array = type_val.as_array()?;

    if type_array.len() != 2 {
        return None;
    }

    let mut has_null = false;
    let mut other_type = None;

    for t in type_array {
        if let Some(t_str) = t.as_str() {
            if t_str == "null" {
                has_null = true;
            } else {
                other_type = Some(t_str.to_string());
            }
        }
    }

    if has_null { other_type } else { None }
}

/// Detect enum pattern from oneOf with const variants
/// Returns Some(variants) if all variants are const strings
pub fn detect_const_enum(schema: &Value) -> Option<Vec<String>> {
    let one_of = schema.get("oneOf")?.as_array()?;

    let mut variants = Vec::new();

    for variant in one_of {
        // Check for {"const": "value"}
        if let Some(const_val) = variant.get("const")
            && let Some(s) = const_val.as_str()
        {
            variants.push(s.to_string());
            continue;
        }
        // If any variant is not a const string, this isn't a simple enum
        return None;
    }

    if variants.is_empty() {
        None
    } else {
        Some(variants)
    }
}

/// Detect simple enum from "enum" array
pub fn detect_simple_enum(schema: &Value) -> Option<Vec<String>> {
    let enum_val = schema.get("enum")?.as_array()?;

    let variants: Vec<String> = enum_val
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    if variants.len() == enum_val.len() && !variants.is_empty() {
        Some(variants)
    } else {
        None
    }
}

/// Get the primary type from a schema
pub fn get_schema_type(schema: &Value) -> Option<&str> {
    schema.get("type")?.as_str()
}

/// Get the description from a schema
pub fn get_description(schema: &Value) -> Option<&str> {
    schema.get("description")?.as_str()
}

/// Get the default value from a schema
pub fn get_default(schema: &Value) -> Option<&Value> {
    schema.get("default")
}

/// Check if schema has a $ref
pub fn get_ref(schema: &Value) -> Option<&str> {
    schema.get("$ref")?.as_str()
}

/// Get properties from an object schema
pub fn get_properties(schema: &Value) -> Option<&serde_json::Map<String, Value>> {
    schema.get("properties")?.as_object()
}

/// Get the items schema for an array
pub fn get_array_items(schema: &Value) -> Option<&Value> {
    schema.get("items")
}

/// Get the prefixItems for a tuple array schema
/// Returns the first item type (for homogeneous tuples like [int, int])
pub fn get_prefix_items_type(schema: &Value) -> Option<&Value> {
    let prefix_items = schema.get("prefixItems")?.as_array()?;
    // Return the first item's schema - for homogeneous tuples this represents all items
    prefix_items.first()
}

/// Get additionalProperties schema
pub fn get_additional_properties(schema: &Value) -> Option<&Value> {
    let ap = schema.get("additionalProperties")?;
    // additionalProperties can be a boolean or a schema
    if ap.is_boolean() { None } else { Some(ap) }
}

/// Get patternProperties schemas (returns first one for simplicity)
pub fn get_pattern_properties(schema: &Value) -> Option<&Value> {
    let pp = schema.get("patternProperties")?.as_object()?;
    // Return the first pattern's schema
    pp.values().next()
}

/// Detect externally tagged union pattern (oneOf with single-property object variants)
/// This is the Serde default representation for Rust enums with data.
/// Example: `oneOf: [{ "Komorebi": {...} }, { "Battery": {...} }]`
/// Returns Some(vec of (tag_name, inner_schema)) if detected
///
/// IMPORTANT: Each variant must have a DIFFERENT tag name. If all variants have the same
/// property name (like "kind"), this is an internally-tagged union, not externally-tagged.
pub fn detect_externally_tagged_union(schema: &Value) -> Option<Vec<(String, Value)>> {
    let one_of = schema.get("oneOf")?.as_array()?;

    // Need at least 2 variants for a tagged union
    if one_of.len() < 2 {
        return None;
    }

    let mut variants = Vec::new();

    for variant in one_of {
        // Check if this variant is an object with exactly one required property
        if let Some(props) = variant.get("properties").and_then(|p| p.as_object())
            && let Some(required) = variant.get("required").and_then(|r| r.as_array())
        {
            // Must have exactly one property and one required field
            if required.len() == 1
                && props.len() == 1
                && let Some(tag) = required[0].as_str()
                && let Some(inner) = props.get(tag)
            {
                variants.push((tag.to_string(), inner.clone()));
                continue;
            }
        }
        // If any variant doesn't match the pattern, this isn't an externally tagged union
        return None;
    }

    if variants.is_empty() {
        return None;
    }

    // Check that all tag names are UNIQUE (externally tagged unions have different keys)
    // If all variants have the same key, this is an internally-tagged union
    let first_tag = &variants[0].0;
    let all_same = variants.iter().all(|(tag, _)| tag == first_tag);
    if all_same {
        // All variants have the same property name - this is internally tagged, not externally
        return None;
    }

    Some(variants)
}

/// Detect internally tagged union pattern (oneOf with shared discriminator field containing const)
/// Example: `oneOf: [{ "kind": "None" }, { "kind": "Bar", ...config }]`
/// Returns Some((discriminator_field, vec of (discriminator_value, variant_schema)))
pub fn detect_internally_tagged_union(schema: &Value) -> Option<(String, Vec<(String, Value)>)> {
    let one_of = schema.get("oneOf")?.as_array()?;

    if one_of.len() < 2 {
        return None;
    }

    // Find common discriminator field with const values
    let mut discriminator_field: Option<String> = None;
    let mut variants = Vec::new();

    for variant in one_of {
        let props = variant.get("properties")?.as_object()?;

        // Look for a property with a const value (the discriminator)
        let mut found_discriminator = false;
        for (key, prop_schema) in props {
            if let Some(const_val) = prop_schema.get("const").and_then(|c| c.as_str()) {
                if discriminator_field.is_none() {
                    discriminator_field = Some(key.clone());
                }
                if discriminator_field.as_ref() == Some(key) {
                    variants.push((const_val.to_string(), variant.clone()));
                    found_discriminator = true;
                    break;
                }
            }
        }

        if !found_discriminator {
            return None;
        }
    }

    let disc = discriminator_field?;
    if variants.len() == one_of.len() {
        Some((disc, variants))
    } else {
        None
    }
}

/// Detect mixed enum + object union (oneOf with string consts AND tagged object variants)
/// Example: DateFormat with "MonthDateYear" (string) OR { "Custom": "..." } (object)
/// Returns Some((enum_variants, object_variants)) if detected
pub fn detect_mixed_enum_object_union(schema: &Value) -> Option<MixedUnionResult> {
    let one_of = schema.get("oneOf")?.as_array()?;

    let mut enum_variants = Vec::new();
    let mut object_variants = Vec::new();

    for variant in one_of {
        // Check for string const
        if variant.get("type").and_then(|t| t.as_str()) == Some("string") {
            if let Some(const_val) = variant.get("const").and_then(|c| c.as_str()) {
                enum_variants.push(const_val.to_string());
                continue;
            }
            // Also handle enum arrays in string type
            if let Some(enum_vals) = variant.get("enum").and_then(|e| e.as_array()) {
                for v in enum_vals {
                    if let Some(s) = v.as_str() {
                        enum_variants.push(s.to_string());
                    }
                }
                continue;
            }
        }

        // Check for object with single required property (externally tagged variant)
        if let Some(props) = variant.get("properties").and_then(|p| p.as_object())
            && let Some(required) = variant.get("required").and_then(|r| r.as_array())
            && required.len() == 1
            && props.len() == 1
            && let Some(tag) = required[0].as_str()
            && let Some(inner) = props.get(tag)
        {
            object_variants.push((tag.to_string(), inner.clone()));
            continue;
        }

        // Variant doesn't match either pattern - could still be valid if we have some of each
    }

    // Only return if we have BOTH enum variants AND object variants
    if !enum_variants.is_empty() && !object_variants.is_empty() {
        Some((enum_variants, object_variants))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resolve_ref_name() {
        assert_eq!(resolve_ref_name("#/$defs/MyType"), Some("MyType"));
        assert_eq!(resolve_ref_name("#/definitions/OldType"), Some("OldType"));
        assert_eq!(resolve_ref_name("invalid"), None);
    }

    #[test]
    fn test_detect_nullable_anyof() {
        let schema = json!({
            "anyOf": [
                {"$ref": "#/$defs/Color"},
                {"type": "null"}
            ]
        });
        let inner = detect_nullable_anyof(&schema).unwrap();
        assert_eq!(inner.get("$ref").unwrap(), "#/$defs/Color");
    }

    #[test]
    fn test_detect_nullable_type_array() {
        let schema = json!({
            "type": ["string", "null"]
        });
        assert_eq!(
            detect_nullable_type_array(&schema),
            Some("string".to_string())
        );
    }

    #[test]
    fn test_detect_const_enum() {
        let schema = json!({
            "oneOf": [
                {"const": "Left"},
                {"const": "Right"},
                {"const": "Center"}
            ]
        });
        let variants = detect_const_enum(&schema).unwrap();
        assert_eq!(variants, vec!["Left", "Right", "Center"]);
    }

    #[test]
    fn test_detect_simple_enum() {
        let schema = json!({
            "enum": ["Red", "Green", "Blue"]
        });
        let variants = detect_simple_enum(&schema).unwrap();
        assert_eq!(variants, vec!["Red", "Green", "Blue"]);
    }

    #[test]
    fn test_collect_definitions() {
        let schema = json!({
            "$defs": {
                "Color": {"type": "string"},
                "Size": {"type": "integer"}
            }
        });
        let defs = collect_definitions(&schema);
        assert_eq!(defs.len(), 2);
        assert!(defs.contains_key("Color"));
        assert!(defs.contains_key("Size"));
    }

    #[test]
    fn test_detect_externally_tagged_union() {
        // Pattern like WidgetConfig: { "Komorebi": {...} } | { "Battery": {...} }
        let schema = json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "Komorebi": {"$ref": "#/$defs/KomorebiConfig"}
                    },
                    "required": ["Komorebi"]
                },
                {
                    "type": "object",
                    "properties": {
                        "Battery": {"$ref": "#/$defs/BatteryConfig"}
                    },
                    "required": ["Battery"]
                }
            ]
        });
        let variants = detect_externally_tagged_union(&schema).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].0, "Komorebi");
        assert_eq!(variants[1].0, "Battery");
    }

    #[test]
    fn test_detect_externally_tagged_union_not_matching() {
        // Not a tagged union - just const enums
        let schema = json!({
            "oneOf": [
                {"type": "string", "const": "Left"},
                {"type": "string", "const": "Right"}
            ]
        });
        assert!(detect_externally_tagged_union(&schema).is_none());
    }

    #[test]
    fn test_detect_internally_tagged_union() {
        // Pattern like Grouping: { "kind": "None" } | { "kind": "Bar", ...config }
        let schema = json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "const": "None"}
                    },
                    "required": ["kind"]
                },
                {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "const": "Bar"},
                        "transparency": {"type": "number"}
                    },
                    "required": ["kind"]
                }
            ]
        });
        let (disc, variants) = detect_internally_tagged_union(&schema).unwrap();
        assert_eq!(disc, "kind");
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].0, "None");
        assert_eq!(variants[1].0, "Bar");
    }

    #[test]
    fn test_detect_mixed_enum_object_union() {
        // Pattern like DateFormat: "MonthDateYear" | { "Custom": "..." }
        let schema = json!({
            "oneOf": [
                {"type": "string", "const": "MonthDateYear"},
                {"type": "string", "const": "YearMonthDate"},
                {
                    "type": "object",
                    "properties": {
                        "Custom": {"type": "string"}
                    },
                    "required": ["Custom"]
                }
            ]
        });
        let (enums, objects) = detect_mixed_enum_object_union(&schema).unwrap();
        assert_eq!(enums, vec!["MonthDateYear", "YearMonthDate"]);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].0, "Custom");
    }
}
