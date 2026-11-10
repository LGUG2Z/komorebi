//! Schema preparation for template rendering
//!
//! This module handles light preparation of the schema before passing it to Tera:
//! - Rendering markdown descriptions to HTML
//! - Generating anchor IDs for navigation
//! - Building the search index

use pulldown_cmark::Parser;
use pulldown_cmark::html;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

/// Prepare a schema value for template rendering
///
/// This function:
/// 1. Clones the schema value
/// 2. Recursively renders all `description` fields as markdown to `description_html`
/// 3. Normalizes non-$defs refs to prevent template errors
/// 4. Returns the modified value ready for Tera
pub fn prepare_for_template(value: &Value) -> Value {
    let mut prepared = value.clone();
    process_value(&mut prepared);
    let root_schema = prepared.clone();
    normalize_refs(&mut prepared, &root_schema);
    detect_wrapper_types(&mut prepared);
    add_wrapper_display_names(&mut prepared);
    annotate_defaults_with_refs(&mut prepared);
    prepared
}

/// Add display names to wrapper types by stripping numeric suffixes
///
/// For wrapper types like `PerAnimationPrefixConfig2` or `PerAnimationPrefixConfig3`,
/// this adds a `_display_name` field with the base name (e.g., `PerAnimationPrefixConfig`).
/// This only applies to types marked with `_wrapper_type: true`, not all types ending in digits.
/// Legitimate type names like `Base16` or `Sha256` are left unchanged.
fn add_wrapper_display_names(schema: &mut Value) {
    if let Some(defs) = schema.get_mut("$defs").and_then(|v| v.as_object_mut()) {
        for (name, def) in defs.iter_mut() {
            // Only strip trailing digits from wrapper types (not all types)
            if def.get("_wrapper_type") == Some(&Value::Bool(true)) {
                let display_name = name.trim_end_matches(|c: char| c.is_ascii_digit());

                // Only add _display_name if it differs from the original name
                if display_name != name
                    && let Some(obj) = def.as_object_mut()
                {
                    obj.insert(
                        "_display_name".to_string(),
                        Value::String(display_name.to_string()),
                    );
                }
            }
        }
    }
}

/// Annotate default values with type references for linking
///
/// When a property has a `default` value and references a type via `$ref` (directly or
/// through `anyOf`/`oneOf` for Option<T> patterns), this adds a `_default_ref` field
/// with the referenced type name. This allows templates to render defaults as links
/// to the type definition.
///
/// For example, if a property has:
/// - `"anyOf": [{"$ref": "#/$defs/MoveBehaviour"}, {"type": "null"}]`
/// - `"default": "Swap"`
///
/// This adds `"_default_ref": "MoveBehaviour"` so the template can render:
/// `Default: [Swap](#movebehaviour)`
fn annotate_defaults_with_refs(schema: &mut Value) {
    annotate_defaults_recursive(schema);
}

fn annotate_defaults_recursive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Check if this object has a default value
            if map.contains_key("default") {
                // Try to find a $ref to a type definition
                if let Some(ref_name) = extract_type_ref(map) {
                    map.insert("_default_ref".to_string(), Value::String(ref_name));
                }
            }

            // Recurse into all nested values
            for v in map.values_mut() {
                annotate_defaults_recursive(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                annotate_defaults_recursive(v);
            }
        }
        _ => {}
    }
}

/// Extract the type name from a $ref, handling various patterns:
/// - Direct `$ref`: `{"$ref": "#/$defs/TypeName"}`
/// - Option<T> via anyOf: `{"anyOf": [{"$ref": "#/$defs/TypeName"}, {"type": "null"}]}`
/// - Option<T> via oneOf: `{"oneOf": [{"$ref": "#/$defs/TypeName"}, {"type": "null"}]}`
fn extract_type_ref(map: &Map<String, Value>) -> Option<String> {
    // Check for direct $ref
    if let Some(ref_value) = map.get("$ref")
        && let Some(ref_str) = ref_value.as_str()
        && let Some(type_name) = ref_str.strip_prefix("#/$defs/")
    {
        return Some(type_name.to_string());
    }

    // Check for anyOf/oneOf Option<T> pattern
    for key in ["anyOf", "oneOf"] {
        if let Some(variants) = map.get(key).and_then(|v| v.as_array()) {
            // Look for the pattern: exactly 2 variants, one null and one $ref
            if variants.len() == 2 {
                let null_count = variants
                    .iter()
                    .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("null"))
                    .count();

                if null_count == 1 {
                    // Find the non-null variant with a $ref
                    for variant in variants {
                        if variant.get("type").and_then(|t| t.as_str()) != Some("null")
                            && let Some(ref_str) = variant.get("$ref").and_then(|v| v.as_str())
                            && let Some(type_name) = ref_str.strip_prefix("#/$defs/")
                        {
                            return Some(type_name.to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

/// Embed usages directly into each $defs entry
///
/// This avoids Tera template lookup issues with special characters in def names
/// (like quotes or empty strings). Each definition will have `_usages` with
/// `config_usages` and `type_references` arrays.
pub fn embed_usages_in_defs(schema: &mut Value, usages: &HashMap<String, TypeUsages>) {
    if let Some(defs) = schema.get_mut("$defs").and_then(|v| v.as_object_mut()) {
        for (name, def) in defs.iter_mut() {
            if let Some(obj) = def.as_object_mut()
                && let Some(type_usages) = usages.get(name)
            {
                obj.insert(
                    "_usages".to_string(),
                    serde_json::to_value(type_usages).unwrap_or(Value::Null),
                );
            }
        }
    }
}

/// Normalize $ref values that don't follow the #/$defs/TypeName pattern
///
/// The template expects all $refs to be in the format #/$defs/TypeName.
/// This function handles other ref patterns:
/// - `#` (root ref): Replaces with the root schema inline
/// - `#/properties/foo` (JSON pointer): Resolves to the target or marks as unresolved
/// - `#/$defs/percent%25field` (URL-encoded): Decodes to the actual $defs name
/// - External URIs: Marks as unresolved reference
fn normalize_refs(value: &mut Value, root_schema: &Value) {
    normalize_refs_recursive(value, root_schema);
}

fn normalize_refs_recursive(value: &mut Value, root_schema: &Value) {
    match value {
        Value::Object(map) => {
            // Check if this object has a $ref that needs normalization
            if let Some(ref_value) = map.get("$ref").cloned()
                && let Some(ref_str) = ref_value.as_str()
                && let Some(normalized) = normalize_single_ref(ref_str, root_schema)
            {
                // Replace this object with the resolved schema
                // But preserve any sibling properties (like description)
                if let Value::Object(resolved_map) = normalized {
                    // Merge: keep original properties, add resolved ones
                    let original_ref = map.remove("$ref");
                    for (k, v) in resolved_map {
                        if !map.contains_key(&k) {
                            map.insert(k, v);
                        }
                    }
                    // If we couldn't resolve, restore the ref but mark it
                    if (map.is_empty()
                        || (!map.contains_key("type")
                            && !map.contains_key("anyOf")
                            && !map.contains_key("oneOf")
                            && !map.contains_key("allOf")
                            && !map.contains_key("properties")
                            && !map.contains_key("enum")
                            && !map.contains_key("const")))
                        && let Some(orig) = original_ref
                    {
                        map.insert("$ref".to_string(), orig);
                    }
                }
            }

            // Recurse into all nested values
            for v in map.values_mut() {
                normalize_refs_recursive(v, root_schema);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_refs_recursive(v, root_schema);
            }
        }
        _ => {}
    }
}

/// Normalize a single $ref string, returning the resolved schema if needed
fn normalize_single_ref(ref_str: &str, root_schema: &Value) -> Option<Value> {
    // Already in expected format - decode URL-encoded characters for $defs lookup
    if let Some(encoded_name) = ref_str.strip_prefix("#/$defs/") {
        // Skip "#/$defs/"

        // Decode JSON pointer escapes: ~1 -> /, ~0 -> ~
        let decoded = encoded_name.replace("~1", "/").replace("~0", "~");

        // Decode URL-encoded characters: %25 -> %, etc.
        let decoded = percent_decode(&decoded);

        // If the decoded name differs from encoded, update the ref
        if decoded != encoded_name {
            // Check if the decoded name exists in $defs
            if let Some(defs) = root_schema.get("$defs").and_then(|v| v.as_object())
                && defs.contains_key(&decoded)
            {
                return Some(serde_json::json!({
                    "$ref": format!("#/$defs/{}", decoded)
                }));
            }
        }
        return None; // No change needed
    }

    // Root reference: #
    if ref_str == "#" {
        // Return a placeholder type - we can't inline the whole root
        return Some(serde_json::json!({
            "type": "object",
            "description": "(recursive reference to root schema)"
        }));
    }

    // JSON pointer reference: #/properties/foo, #/items, etc.
    if ref_str.starts_with("#/") {
        if let Some(resolved) = resolve_json_pointer(ref_str, root_schema) {
            return Some(resolved.clone());
        }
        // Couldn't resolve - return a placeholder
        return Some(serde_json::json!({
            "description": format!("(unresolved reference: {})", ref_str)
        }));
    }

    // External URI reference (http://, https://, urn:, file://, etc.)
    if ref_str.contains("://") || ref_str.starts_with("urn:") {
        return Some(serde_json::json!({
            "description": format!("(external reference: {})", ref_str)
        }));
    }

    // Relative URI reference (e.g., "other-schema.json#/$defs/Foo")
    if ref_str.contains('#') || ref_str.ends_with(".json") {
        return Some(serde_json::json!({
            "description": format!("(external reference: {})", ref_str)
        }));
    }

    None // Unknown format, leave as-is
}

/// Decode percent-encoded characters in a string
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            // Try to read two hex digits
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2
                && let Ok(byte) = u8::from_str_radix(&hex, 16)
            {
                result.push(byte as char);
                continue;
            }
            // If decoding failed, keep the original
            result.push('%');
            result.push_str(&hex);
        } else {
            result.push(c);
        }
    }

    result
}

/// Resolve a JSON pointer within the schema
fn resolve_json_pointer<'a>(pointer: &str, schema: &'a Value) -> Option<&'a Value> {
    // Remove leading #
    let pointer = pointer.strip_prefix('#')?;

    if pointer.is_empty() {
        return Some(schema);
    }

    // Remove leading /
    let pointer = pointer.strip_prefix('/')?;

    let mut current = schema;
    for segment in pointer.split('/') {
        // Decode JSON pointer escapes
        let decoded = segment.replace("~1", "/").replace("~0", "~");
        // Also decode percent-encoding
        let decoded = percent_decode(&decoded);

        current = match current {
            Value::Object(map) => map.get(&decoded)?,
            Value::Array(arr) => {
                let index: usize = decoded.parse().ok()?;
                arr.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}

/// Recursively process a value, rendering markdown descriptions
fn process_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Render description if present
            if let Some(Value::String(desc)) = map.get("description") {
                let html = render_markdown(desc);
                map.insert("description_html".to_string(), Value::String(html));
            }

            // Recurse into all nested values
            for v in map.values_mut() {
                process_value(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                process_value(v);
            }
        }
        _ => {}
    }
}

/// Detect and annotate wrapper type patterns in the schema
///
/// A wrapper type is an anyOf with exactly 2 variants where:
/// - One variant is an object with additionalProperties of type T
/// - The other variant is directly type T
///
/// This pattern is common for types like `PerAnimationPrefixConfig<T>` which
/// can be either a map of animation prefixes to T, or directly a T value.
fn detect_wrapper_types(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Check for wrapper type pattern in anyOf
            if let Some(any_of) = map.get("anyOf").and_then(|v| v.as_array())
                && let Some(inner) = detect_wrapper_pattern(any_of)
            {
                map.insert("_wrapper_type".to_string(), Value::Bool(true));
                map.insert("_wrapper_inner".to_string(), inner);
            }

            // Recurse into all nested values
            for v in map.values_mut() {
                detect_wrapper_types(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                detect_wrapper_types(v);
            }
        }
        _ => {}
    }
}

/// Check if anyOf array matches the wrapper type pattern
///
/// Returns the inner type if the pattern matches, None otherwise.
fn detect_wrapper_pattern(any_of: &[Value]) -> Option<Value> {
    // Must have exactly 2 variants
    if any_of.len() != 2 {
        return None;
    }

    let first = &any_of[0];
    let second = &any_of[1];

    // Skip if either variant is null (that's Option<T>, not wrapper)
    if is_null_type(first) || is_null_type(second) {
        return None;
    }

    // Try first as map variant, second as inner type
    if let Some(map_inner) = get_map_inner_type(first)
        && types_are_equal(map_inner, second)
    {
        return Some(second.clone());
    }

    // Try second as map variant, first as inner type
    if let Some(map_inner) = get_map_inner_type(second)
        && types_are_equal(map_inner, first)
    {
        return Some(first.clone());
    }

    None
}

/// Check if a schema represents the null type
fn is_null_type(schema: &Value) -> bool {
    schema
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t == "null")
        .unwrap_or(false)
}

/// Extract the inner type from a map schema
///
/// Handles two patterns:
/// 1. Old pattern (schemars 1.1): object with additionalProperties: { schema }
/// 2. New pattern (schemars 1.2): object with properties: { key: { schema } } + additionalProperties: false
///    This is used for HashMap<EnumKey, T> where enum keys become explicit property names
fn get_map_inner_type(schema: &Value) -> Option<&Value> {
    let obj = schema.as_object()?;

    // Must be type: "object"
    let type_val = obj.get("type")?;
    if type_val.as_str()? != "object" {
        return None;
    }

    // Pattern 1: additionalProperties as object schema (schemars 1.1)
    if let Some(additional) = obj.get("additionalProperties")
        && additional.is_object()
    {
        return Some(additional);
    }

    // Pattern 2: enum-keyed map (schemars 1.2)
    // Has properties with additionalProperties: false
    if let Some(props) = obj.get("properties").and_then(|v| v.as_object())
        && obj.get("additionalProperties") == Some(&Value::Bool(false))
        && !props.is_empty()
    {
        // Return the first property's value as the inner type
        // (all properties should have the same schema for enum-keyed maps)
        if let Some((_, first_prop)) = props.iter().next() {
            return Some(first_prop);
        }
    }

    None
}

/// Check if two type schemas are equivalent
///
/// Handles:
/// - $ref comparison (by ref string)
/// - Primitive type comparison (by type string)
/// - Deep equality for complex types
fn types_are_equal(a: &Value, b: &Value) -> bool {
    // Handle $ref comparison
    if let (Some(ref_a), Some(ref_b)) = (
        a.get("$ref").and_then(|v| v.as_str()),
        b.get("$ref").and_then(|v| v.as_str()),
    ) {
        return ref_a == ref_b;
    }

    // Handle primitive type comparison
    if let (Some(type_a), Some(type_b)) = (
        a.get("type").and_then(|v| v.as_str()),
        b.get("type").and_then(|v| v.as_str()),
    ) {
        // For primitive types, just compare the type string
        // (ignoring format, constraints, etc. for matching purposes)
        return type_a == type_b;
    }

    // Deep equality for complex types
    a == b
}

/// Render markdown text to HTML
pub fn render_markdown(text: &str) -> String {
    let parser = Parser::new(text);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Strip markdown formatting from text, keeping only the plain text content.
/// Useful for HTML <title> tags where markdown syntax shouldn't appear.
pub fn strip_markdown(text: &str) -> String {
    let parser = Parser::new(text);
    let mut plain_text = String::new();
    for event in parser {
        if let pulldown_cmark::Event::Text(t) | pulldown_cmark::Event::Code(t) = event {
            plain_text.push_str(&t);
        }
    }
    plain_text
}

/// Information about a definition for the sidebar navigation
#[derive(Debug, Clone, Serialize)]
pub struct DefinitionInfo {
    pub name: String,
    pub anchor: String,
    pub description: Option<String>,
    pub display_name: Option<String>,
}

/// An outline entry for hierarchical navigation
#[derive(Debug, Clone, Serialize)]
pub struct OutlineEntry {
    pub name: String,
    pub anchor: String,
    pub children: Vec<OutlineEntry>,
}

/// A search index entry
#[derive(Debug, Clone, Serialize)]
pub struct SearchEntry {
    pub name: String,
    pub path: String,
    pub anchor: String,
    pub entry_type: String,
}

/// A usage of a type in the Configuration section
#[derive(Debug, Clone, Serialize)]
pub struct ConfigUsage {
    /// jq-style path like ".monitors[].workspaces[].layout"
    pub path: String,
    /// HTML anchor for linking to the usage location (no leading dash)
    pub anchor: String,
}

/// A reference to a type from another type definition
#[derive(Debug, Clone, Serialize)]
pub struct TypeReference {
    /// Name of the referencing type, e.g., "IdWithIdentifier"
    pub type_name: String,
    /// Anchor to the type definition
    pub anchor: String,
}

/// All usages of a type
#[derive(Debug, Clone, Serialize)]
pub struct TypeUsages {
    /// Usages in Configuration section
    pub config_usages: Vec<ConfigUsage>,
    /// References from other $defs types
    pub type_references: Vec<TypeReference>,
}

/// Navigation data extracted from the schema
#[derive(Debug, Clone, Serialize)]
pub struct NavigationData {
    pub outline: Vec<OutlineEntry>,
    pub definitions: Vec<DefinitionInfo>,
    pub search_index: Vec<SearchEntry>,
    /// Map from type name to its usages
    pub usages: HashMap<String, TypeUsages>,
    /// Map from type name to display name (for wrapper types with numeric suffixes)
    pub display_names: HashMap<String, String>,
}

/// Maximum depth for outline tree to prevent infinite recursion
const MAX_OUTLINE_DEPTH: usize = 3;

/// Build a hierarchical outline tree from schema properties
fn build_outline_tree(
    schema: &Value,
    defs: Option<&Map<String, Value>>,
    parent_path: &str,
    depth: usize,
) -> Vec<OutlineEntry> {
    if depth >= MAX_OUTLINE_DEPTH {
        return Vec::new();
    }

    let mut entries = Vec::new();

    // Resolve the schema, getting both the effective schema and the anchor base
    // When we follow a $ref, the anchor base becomes the type name
    let (effective_schema, anchor_base) = resolve_schema_for_outline(schema, defs, parent_path);

    // Check if this is a oneOf - show all variants as alternatives
    if let Some(one_of) = effective_schema.get("oneOf").and_then(|v| v.as_array()) {
        for (i, variant) in one_of.iter().enumerate() {
            // Get variant label: prefer title, then look for a const discriminator, then fall back to "Variant N"
            let variant_label = variant
                .get("title")
                .and_then(|t| t.as_str())
                .map(String::from)
                .or_else(|| {
                    // Look for a const property that could serve as a discriminator (e.g., "palette": {"const": "Catppuccin"})
                    variant
                        .get("properties")
                        .and_then(|p| p.as_object())
                        .and_then(|props| {
                            props.values().find_map(|prop| {
                                prop.get("const").and_then(|c| c.as_str()).map(String::from)
                            })
                        })
                })
                .unwrap_or_else(|| format!("Variant {}", i + 1));

            // Build the variant path to match template-generated IDs
            // Template uses: path ~ ".oneOf[" ~ loop.index0 ~ "]"
            let variant_path = format!("{}.oneOf[{}]", anchor_base, i);

            // Build children for this variant
            let variant_children = build_outline_tree(variant, defs, &variant_path, depth + 1);

            if !variant_children.is_empty() {
                entries.push(OutlineEntry {
                    name: format!("⊕ {}", variant_label), // ⊕ indicates "one of"
                    // Link to the type definition since individual variants don't have anchors
                    anchor: make_anchor(&anchor_base),
                    children: variant_children,
                });
            }
        }
        return entries;
    }

    if let Some(props) = effective_schema
        .get("properties")
        .and_then(|v| v.as_object())
    {
        for (name, prop) in props {
            // Skip properties with const values (discriminator fields in internally tagged enums)
            // These are shown as variant headings in templates, not as rendered properties
            if prop.get("const").is_some() {
                continue;
            }

            // Build the anchor path using the resolved anchor base
            let child_anchor_path = if anchor_base.is_empty() {
                name.clone()
            } else {
                format!("{}.{}", anchor_base, name)
            };

            // Recursively build children
            let children = build_outline_tree(prop, defs, &child_anchor_path, depth + 1);

            entries.push(OutlineEntry {
                name: name.clone(),
                anchor: make_anchor(&child_anchor_path),
                children,
            });
        }
        // Sort alphabetically
        entries.sort_by_key(|a| a.name.to_lowercase());
    }

    entries
}

/// Resolve schema following $refs, arrays, and Option<T>, returning (effective_schema, anchor_base)
/// When we follow a $ref, anchor_base becomes the type name so links go to the type definition
/// For true union types (anyOf with multiple non-null variants), we stop and treat as leaf
fn resolve_schema_for_outline<'a>(
    schema: &'a Value,
    defs: Option<&'a Map<String, Value>>,
    current_path: &str,
) -> (&'a Value, String) {
    // Handle $ref - use the ref name as anchor base
    if let Some(ref_path) = schema.get("$ref").and_then(|v| v.as_str()) {
        let ref_name = ref_path.trim_start_matches("#/$defs/");
        if let Some(def) = defs.and_then(|d| d.get(ref_name)) {
            // Recursively resolve in case the def itself has a $ref or anyOf
            // Use the ref_name as the new anchor base
            return resolve_schema_for_outline(def, defs, ref_name);
        }
        return (schema, current_path.to_string());
    }

    // Handle array - unwrap to items schema
    if is_array_type(schema)
        && let Some(items) = schema.get("items")
    {
        return resolve_schema_for_outline(items, defs, current_path);
    }

    // Handle anyOf - only unwrap if it's Option<T> (exactly 2 variants, one null)
    // For true unions (multiple non-null variants), don't unwrap - treat as leaf
    if let Some(any_of) = schema.get("anyOf").and_then(|v| v.as_array())
        && any_of.len() == 2
    {
        let null_count = any_of
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("null"))
            .count();

        // Only unwrap if exactly one null variant (Option<T> pattern)
        if null_count == 1 {
            for variant in any_of {
                let is_null = variant.get("type").and_then(|t| t.as_str()) == Some("null");
                if !is_null {
                    return resolve_schema_for_outline(variant, defs, current_path);
                }
            }
        }
    }
    // For true unions (0 or 2+ null variants, or 3+ total variants), don't unwrap

    // Inline object or leaf type - keep current path
    (schema, current_path.to_string())
}

/// Check if schema represents an array type
fn is_array_type(schema: &Value) -> bool {
    // Check type: "array"
    if schema.get("type").and_then(|t| t.as_str()) == Some("array") {
        return true;
    }
    // Check type: ["array", "null"] (Option<Vec<T>>)
    if let Some(type_arr) = schema.get("type").and_then(|v| v.as_array())
        && type_arr.iter().any(|t| t.as_str() == Some("array"))
    {
        return true;
    }
    false
}

/// Check if schema is an enum-keyed map (schemars 1.2 pattern)
///
/// This pattern is generated for HashMap<EnumKey, T> where enum variant names
/// become explicit property keys with additionalProperties: false.
/// These properties are rendered inline in templates (Pattern 7), not as separate headings.
fn is_enum_keyed_map(schema: &Value) -> bool {
    if let Some(obj) = schema.as_object()
        && obj.get("type").and_then(|t| t.as_str()) == Some("object")
        && obj.get("additionalProperties") == Some(&Value::Bool(false))
        && let Some(props) = obj.get("properties").and_then(|v| v.as_object())
    {
        return !props.is_empty();
    }
    false
}

/// Collect all usages of type definitions ($refs) in the Configuration section
///
/// Recursively traverses the schema and records where each type is referenced,
/// building jq-style paths for each usage location. Follows $refs transitively
/// to collect usages for types nested inside other types.
///
/// Parameters:
/// - `current_type`: When inside a followed $ref, this is the type name we're inside.
///   Used to build correct anchors that point to the type definition's rendered properties.
/// - `type_relative_path`: When inside a type, this tracks the path relative to that type
///   (e.g., "workspaces" inside MonitorConfig), used for building anchors.
fn collect_config_usages(
    schema: &Value,
    path: String,
    usages: &mut HashMap<String, Vec<ConfigUsage>>,
    defs: Option<&Map<String, Value>>,
    visited: &mut HashSet<String>,
    current_type: Option<&str>,
    type_relative_path: String,
) {
    // Check for $ref at current level
    if let Some(ref_path) = schema.get("$ref").and_then(|v| v.as_str()) {
        let type_name = ref_path.trim_start_matches("#/$defs/").to_string();

        // Build the anchor based on whether we're inside a type or at config root
        let anchor = if let Some(parent_type) = current_type {
            // Inside a type definition - anchor points to property within that type
            // e.g., MonitorConfig.workspaces -> "monitorconfig-workspaces"
            // e.g., KomorebiTheme.oneOf[0].bar_accent -> "komorebitheme-oneof[0]-bar_accent"
            // Note: Keep [N] for variant indices, but remove empty [] from array notation
            let clean_path = type_relative_path.replace("[]", "");

            // If the path is ONLY a variant index (e.g., "oneOf[1]"), the $ref is at variant root.
            // In this case, link to the parent type, not the non-existent variant anchor.
            // The template doesn't render separate heading IDs for bare $ref variants.
            let is_variant_root_ref = (clean_path.starts_with("oneOf[")
                || clean_path.starts_with("anyOf[")
                || clean_path.starts_with("allOf["))
                && !clean_path.contains('.');

            if clean_path.is_empty() || is_variant_root_ref {
                make_anchor(parent_type)
            } else {
                make_anchor(&format!("{}.{}", parent_type, clean_path))
            }
        } else {
            // At config root - anchor points to config section property
            // e.g., .monitors[] -> "monitors" (template IDs don't have [])
            // Remove all bracket notation for config paths
            // Also remove [*] notation from patternProperties/additionalProperties
            let clean_path = path
                .trim_start_matches('.')
                .replace("[*]", "")
                .replace("[]", "")
                .replace("[", "-")
                .replace("]", "");
            make_anchor(&clean_path)
        };

        let usage = ConfigUsage {
            path: if path.is_empty() {
                ".".to_string()
            } else {
                path.clone()
            },
            anchor,
        };
        usages.entry(type_name.clone()).or_default().push(usage);

        // Follow the $ref into the type definition (if not already visited)
        // When following, we enter the new type with an empty relative path
        if !visited.contains(&type_name)
            && let Some(def) = defs.and_then(|d| d.get(&type_name))
        {
            visited.insert(type_name.clone());
            collect_config_usages(
                def,
                path,
                usages,
                defs,
                visited,
                Some(&type_name),
                String::new(), // Reset relative path when entering new type
            );
            visited.remove(&type_name); // Allow revisiting from different paths
        }
        return; // $ref schema has no other content to process
    }

    // Recurse into properties
    // Skip enum-keyed maps (schemars 1.2) - they render inline without heading anchors
    if !is_enum_keyed_map(schema)
        && let Some(props) = schema.get("properties").and_then(|v| v.as_object())
    {
        for (prop_name, prop_schema) in props {
            let new_path = if path.is_empty() {
                format!(".{}", prop_name)
            } else {
                format!("{}.{}", path, prop_name)
            };
            let new_type_relative = if type_relative_path.is_empty() {
                prop_name.clone()
            } else {
                format!("{}.{}", type_relative_path, prop_name)
            };
            collect_config_usages(
                prop_schema,
                new_path,
                usages,
                defs,
                visited,
                current_type,
                new_type_relative,
            );
        }
    }

    // Recurse into array items
    if let Some(items) = schema.get("items") {
        let new_path = format!("{}[]", path);
        // For arrays, the relative path stays the same (items are part of the array property)
        collect_config_usages(
            items,
            new_path,
            usages,
            defs,
            visited,
            current_type,
            type_relative_path.clone(),
        );
    }

    // Recurse into prefixItems (tuples)
    if let Some(prefix_items) = schema.get("prefixItems").and_then(|v| v.as_array()) {
        for (i, item) in prefix_items.iter().enumerate() {
            let new_path = format!("{}[{}]", path, i);
            collect_config_usages(
                item,
                new_path,
                usages,
                defs,
                visited,
                current_type,
                type_relative_path.clone(),
            );
        }
    }

    // Recurse into anyOf/oneOf/allOf variants
    // Only add variant indices to path when the variant has properties (will render sub-IDs)
    // For $ref or primitive variants, the template doesn't render separate IDs
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(variants) = schema.get(key).and_then(|v| v.as_array()) {
            for (i, variant) in variants.iter().enumerate() {
                // Only add variant index if:
                // 1. We're inside a type (current_type.is_some())
                // 2. We're at the type root level (type_relative_path.is_empty())
                // 3. The variant has properties (will render sub-IDs in template)
                let variant_has_properties = variant.get("properties").is_some();
                let new_type_relative = if current_type.is_some()
                    && type_relative_path.is_empty()
                    && variant_has_properties
                {
                    format!("{}[{}]", key, i)
                } else {
                    type_relative_path.clone()
                };
                collect_config_usages(
                    variant,
                    path.clone(),
                    usages,
                    defs,
                    visited,
                    current_type,
                    new_type_relative,
                );
            }
        }
    }

    // Recurse into patternProperties
    if let Some(patterns) = schema.get("patternProperties").and_then(|v| v.as_object()) {
        for (_pattern, prop_schema) in patterns {
            let new_path = format!("{}[*]", path);
            collect_config_usages(
                prop_schema,
                new_path,
                usages,
                defs,
                visited,
                current_type,
                type_relative_path.clone(),
            );
        }
    }

    // Recurse into additionalProperties
    if let Some(additional) = schema.get("additionalProperties")
        && additional.is_object()
    {
        let new_path = format!("{}[*]", path);
        collect_config_usages(
            additional,
            new_path,
            usages,
            defs,
            visited,
            current_type,
            type_relative_path.clone(),
        );
    }
}

/// Collect references to types from within $defs
///
/// For each type definition, find all $refs it contains and record them
/// as references from that type to the target type.
fn collect_type_references(defs: &Map<String, Value>) -> HashMap<String, Vec<TypeReference>> {
    let mut references: HashMap<String, Vec<TypeReference>> = HashMap::new();

    for (def_name, def_schema) in defs {
        collect_refs_in_schema(def_schema, def_name, &mut references);
    }

    // Sort and deduplicate references
    for ref_list in references.values_mut() {
        ref_list.sort_by(|a, b| a.type_name.cmp(&b.type_name));
        ref_list.dedup_by(|a, b| a.type_name == b.type_name);
    }

    references
}

/// Recursively find all $refs in a schema and record them as references
fn collect_refs_in_schema(
    schema: &Value,
    source_type: &str,
    references: &mut HashMap<String, Vec<TypeReference>>,
) {
    if let Some(ref_path) = schema.get("$ref").and_then(|v| v.as_str()) {
        let target_type = ref_path.trim_start_matches("#/$defs/").to_string();
        if target_type != source_type {
            // Don't self-reference
            let reference = TypeReference {
                type_name: source_type.to_string(),
                anchor: make_anchor(source_type), // Links to type definition
            };
            references.entry(target_type).or_default().push(reference);
        }
    }

    // Recurse into all nested structures
    if let Some(obj) = schema.as_object() {
        for value in obj.values() {
            collect_refs_in_schema(value, source_type, references);
        }
    }
    if let Some(arr) = schema.as_array() {
        for value in arr {
            collect_refs_in_schema(value, source_type, references);
        }
    }
}

/// Build navigation data from a schema value
pub fn build_navigation(schema: &Value) -> NavigationData {
    let mut definitions = Vec::new();
    let mut search_index = Vec::new();

    // Get $defs for resolving references
    let defs = schema.get("$defs").and_then(|v| v.as_object());

    // Build hierarchical outline from top-level properties
    let outline = build_outline_tree(schema, defs, "", 0);

    // Also collect for search index
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        collect_properties(props, "", &mut search_index);
    }

    // Extract definitions from $defs
    if let Some(defs) = schema.get("$defs").and_then(|v| v.as_object()) {
        for (name, def) in defs {
            let anchor = make_anchor(name);
            let description = def.get("description").and_then(|v| v.as_str()).map(|s| {
                // Truncate long descriptions for the sidebar
                if s.len() > 100 {
                    format!("{}...", &s[..97])
                } else {
                    s.to_string()
                }
            });

            let display_name = def
                .get("_display_name")
                .and_then(|v| v.as_str())
                .map(String::from);

            definitions.push(DefinitionInfo {
                name: name.clone(),
                anchor: anchor.clone(),
                description,
                display_name,
            });

            search_index.push(SearchEntry {
                name: name.clone(),
                path: name.clone(),
                anchor,
                entry_type: "definition".to_string(),
            });
        }
    }

    // Build display_names map (type_name -> display_name for wrapper types)
    let mut display_names: HashMap<String, String> = HashMap::new();
    for def in &definitions {
        if let Some(display) = &def.display_name {
            display_names.insert(def.name.clone(), display.clone());
        }
    }

    // Sort definitions alphabetically
    definitions.sort_by_key(|a| a.name.to_lowercase());

    // Deduplicate definitions by display_name (for wrapper types with same base name)
    // Keep only the first occurrence of each display_name
    let mut seen_display_names: HashSet<String> = HashSet::new();
    definitions.retain(|def| {
        let display = def.display_name.as_ref().unwrap_or(&def.name);
        if seen_display_names.contains(display) {
            false // Skip duplicate
        } else {
            seen_display_names.insert(display.clone());
            true // Keep first occurrence
        }
    });

    // Collect configuration usages (from main schema, following $refs transitively)
    let mut config_usages: HashMap<String, Vec<ConfigUsage>> = HashMap::new();
    let mut visited: HashSet<String> = HashSet::new();
    collect_config_usages(
        schema,
        String::new(),
        &mut config_usages,
        defs,
        &mut visited,
        None,          // not inside any type yet
        String::new(), // no type-relative path yet
    );

    // Sort config usages alphabetically by path
    for usage_list in config_usages.values_mut() {
        usage_list.sort_by(|a, b| a.path.cmp(&b.path));
    }

    // Collect type references from $defs
    let type_references = if let Some(defs) = schema.get("$defs").and_then(|v| v.as_object()) {
        collect_type_references(defs)
    } else {
        HashMap::new()
    };

    // Merge into TypeUsages for each type
    let mut usages: HashMap<String, TypeUsages> = HashMap::new();
    if let Some(defs) = schema.get("$defs").and_then(|v| v.as_object()) {
        for type_name in defs.keys() {
            usages.insert(
                type_name.clone(),
                TypeUsages {
                    config_usages: config_usages.remove(type_name).unwrap_or_default(),
                    type_references: type_references.get(type_name).cloned().unwrap_or_default(),
                },
            );
        }
    }

    NavigationData {
        outline,
        definitions,
        search_index,
        usages,
        display_names,
    }
}

/// Recursively collect properties for the search index
fn collect_properties(props: &Map<String, Value>, prefix: &str, index: &mut Vec<SearchEntry>) {
    for (name, prop) in props {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };

        let anchor = make_anchor(&path);

        index.push(SearchEntry {
            name: name.clone(),
            path: path.clone(),
            anchor,
            entry_type: "property".to_string(),
        });

        // Recurse into nested properties
        if let Some(nested_props) = prop.get("properties").and_then(|v| v.as_object()) {
            collect_properties(nested_props, &path, index);
        }

        // Handle anyOf/oneOf which might contain objects with properties
        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(variants) = prop.get(key).and_then(|v| v.as_array()) {
                for variant in variants {
                    if let Some(nested_props) =
                        variant.get("properties").and_then(|v| v.as_object())
                    {
                        collect_properties(nested_props, &path, index);
                    }
                }
            }
        }
    }
}

/// Create a URL-safe anchor ID from a name
pub fn make_anchor(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '[' || c == ']' {
                c.to_ascii_lowercase()
            } else if c == '.' || c == ' ' {
                '-'
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_render_markdown() {
        let md = "This is **bold** and `code`";
        let html = render_markdown(md);
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn test_render_markdown_with_links() {
        let md = "See [docs](https://example.com)";
        let html = render_markdown(md);
        assert!(html.contains("<a href=\"https://example.com\">docs</a>"));
    }

    #[test]
    fn test_prepare_for_template() {
        let schema = json!({
            "title": "Test",
            "description": "A **bold** description",
            "properties": {
                "name": {
                    "description": "The `name` field"
                }
            }
        });

        let prepared = prepare_for_template(&schema);

        assert!(prepared.get("description_html").is_some());
        assert!(
            prepared["description_html"]
                .as_str()
                .unwrap()
                .contains("<strong>bold</strong>")
        );

        assert!(prepared["properties"]["name"]["description_html"].is_string());
    }

    #[test]
    fn test_make_anchor() {
        assert_eq!(make_anchor("MyType"), "mytype");
        assert_eq!(make_anchor("my.nested.prop"), "my-nested-prop");
        assert_eq!(make_anchor("with spaces"), "with-spaces");
        assert_eq!(make_anchor("with_underscore"), "with_underscore");
    }

    #[test]
    fn test_build_navigation() {
        let schema = json!({
            "title": "Test",
            "$defs": {
                "TypeA": {"type": "object", "description": "Type A description"},
                "TypeB": {"type": "string"}
            },
            "properties": {
                "config": {
                    "properties": {
                        "nested": {"type": "string"}
                    }
                }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.definitions.len(), 2);
        assert!(nav.definitions.iter().any(|d| d.name == "TypeA"));
        assert!(nav.definitions.iter().any(|d| d.name == "TypeB"));

        // Check search index includes properties
        assert!(nav.search_index.iter().any(|e| e.name == "config"));
        assert!(nav.search_index.iter().any(|e| e.path == "config.nested"));
    }

    #[test]
    fn test_build_navigation_outline() {
        let schema = json!({
            "title": "Test",
            "properties": {
                "zebra": { "type": "string" },
                "alpha": { "type": "number" },
                "beta": { "type": "boolean" }
            }
        });

        let nav = build_navigation(&schema);

        // Should have 3 outline entries
        assert_eq!(nav.outline.len(), 3);

        // Should be sorted alphabetically
        assert_eq!(nav.outline[0].name, "alpha");
        assert_eq!(nav.outline[1].name, "beta");
        assert_eq!(nav.outline[2].name, "zebra");

        // Check anchors
        assert_eq!(nav.outline[0].anchor, "alpha");
        assert_eq!(nav.outline[1].anchor, "beta");
        assert_eq!(nav.outline[2].anchor, "zebra");

        // Primitive types should have no children
        assert!(nav.outline[0].children.is_empty());
    }

    #[test]
    fn test_outline_array_shows_item_properties() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "ChildType": {
                    "type": "object",
                    "properties": {
                        "nested_prop": { "type": "string" }
                    }
                }
            },
            "properties": {
                "parent": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/ChildType" }
                }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.outline.len(), 1);
        assert_eq!(nav.outline[0].name, "parent");
        // Arrays should show their item type's properties
        assert_eq!(nav.outline[0].children.len(), 1);
        assert_eq!(nav.outline[0].children[0].name, "nested_prop");
        // The anchor should point to the type definition, not the parent path
        assert_eq!(nav.outline[0].children[0].anchor, "childtype-nested_prop");
    }

    #[test]
    fn test_outline_union_type_is_leaf() {
        // Union types (anyOf with multiple non-null variants) should be leaf nodes
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Colour": {
                    "anyOf": [
                        { "$ref": "#/$defs/Rgb" },
                        { "$ref": "#/$defs/Hex" }
                    ]
                },
                "Rgb": {
                    "type": "object",
                    "properties": {
                        "r": { "type": "integer" },
                        "g": { "type": "integer" },
                        "b": { "type": "integer" }
                    }
                },
                "Hex": {
                    "type": "string"
                }
            },
            "properties": {
                "color": { "$ref": "#/$defs/Colour" }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.outline.len(), 1);
        assert_eq!(nav.outline[0].name, "color");
        // Union type should be a leaf - don't drill into its variants
        assert!(nav.outline[0].children.is_empty());
    }

    #[test]
    fn test_outline_unwraps_optional() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "InnerType": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" }
                    }
                }
            },
            "properties": {
                "optional_obj": {
                    "anyOf": [
                        { "$ref": "#/$defs/InnerType" },
                        { "type": "null" }
                    ]
                }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.outline[0].name, "optional_obj");
        // Optional should be transparent - show InnerType's properties as children
        assert_eq!(nav.outline[0].children.len(), 1);
        assert_eq!(nav.outline[0].children[0].name, "value");
    }

    #[test]
    fn test_outline_one_of_shows_variants_with_title() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "ThemeChoice": {
                    "oneOf": [
                        {
                            "title": "CatppuccinTheme",
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "accent": { "type": "string" }
                            }
                        },
                        {
                            "title": "CustomTheme",
                            "type": "object",
                            "properties": {
                                "colors": { "type": "object" },
                                "font": { "type": "string" }
                            }
                        }
                    ]
                }
            },
            "properties": {
                "theme": { "$ref": "#/$defs/ThemeChoice" }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.outline.len(), 1);
        assert_eq!(nav.outline[0].name, "theme");

        // Should have 2 variant children (one for each oneOf variant)
        assert_eq!(nav.outline[0].children.len(), 2);

        // First variant should use title
        assert_eq!(nav.outline[0].children[0].name, "⊕ CatppuccinTheme");
        assert_eq!(nav.outline[0].children[0].children.len(), 2); // name, accent
        // Anchor should point to the type definition
        assert_eq!(nav.outline[0].children[0].anchor, "themechoice");

        // Second variant
        assert_eq!(nav.outline[0].children[1].name, "⊕ CustomTheme");
        assert_eq!(nav.outline[0].children[1].children.len(), 2); // colors, font
    }

    #[test]
    fn test_outline_one_of_uses_const_discriminator() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "ThemeChoice": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "palette": { "const": "Catppuccin" },
                                "name": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "properties": {
                                "palette": { "const": "Base16" },
                                "name": { "type": "string" }
                            }
                        }
                    ]
                }
            },
            "properties": {
                "theme": { "$ref": "#/$defs/ThemeChoice" }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.outline[0].children.len(), 2);

        // Should use const value as discriminator
        assert_eq!(nav.outline[0].children[0].name, "⊕ Catppuccin");
        assert_eq!(nav.outline[0].children[1].name, "⊕ Base16");
    }

    #[test]
    fn test_outline_one_of_fallback_to_variant_n() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Choice": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "option_a": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "properties": {
                                "option_b": { "type": "number" }
                            }
                        }
                    ]
                }
            },
            "properties": {
                "selection": { "$ref": "#/$defs/Choice" }
            }
        });

        let nav = build_navigation(&schema);

        assert_eq!(nav.outline[0].name, "selection");
        assert_eq!(nav.outline[0].children.len(), 2);

        // Without title or const, should use "Variant N"
        assert_eq!(nav.outline[0].children[0].name, "⊕ Variant 1");
        assert_eq!(nav.outline[0].children[1].name, "⊕ Variant 2");
    }

    #[test]
    fn test_outline_depth_limit() {
        // Create a deeply nested schema
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Level1": {
                    "type": "object",
                    "properties": {
                        "level2": { "$ref": "#/$defs/Level2" }
                    }
                },
                "Level2": {
                    "type": "object",
                    "properties": {
                        "level3": { "$ref": "#/$defs/Level3" }
                    }
                },
                "Level3": {
                    "type": "object",
                    "properties": {
                        "level4": { "$ref": "#/$defs/Level4" }
                    }
                },
                "Level4": {
                    "type": "object",
                    "properties": {
                        "deep": { "type": "string" }
                    }
                }
            },
            "properties": {
                "root": { "$ref": "#/$defs/Level1" }
            }
        });

        let nav = build_navigation(&schema);

        // Should have root
        assert_eq!(nav.outline.len(), 1);
        assert_eq!(nav.outline[0].name, "root");

        // Should have level2 as child
        assert_eq!(nav.outline[0].children.len(), 1);
        assert_eq!(nav.outline[0].children[0].name, "level2");

        // Should have level3 as grandchild
        assert_eq!(nav.outline[0].children[0].children.len(), 1);
        assert_eq!(nav.outline[0].children[0].children[0].name, "level3");

        // Should stop at max depth (3) - level4 should not have children
        assert!(nav.outline[0].children[0].children[0].children.is_empty());
    }

    #[test]
    fn test_config_usages_collection() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Layout": {
                    "type": "string",
                    "enum": ["horizontal", "vertical"]
                },
                "Workspace": {
                    "type": "object"
                }
            },
            "properties": {
                "workspaces": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/Workspace" }
                },
                "default_layout": { "$ref": "#/$defs/Layout" },
                "alternate_layout": { "$ref": "#/$defs/Layout" }
            }
        });

        let nav = build_navigation(&schema);

        // Layout should have 2 config usages
        assert!(nav.usages.contains_key("Layout"));
        let layout_usages = &nav.usages["Layout"];
        assert_eq!(layout_usages.config_usages.len(), 2);

        // Check paths are jq-style and sorted
        let paths: Vec<&str> = layout_usages
            .config_usages
            .iter()
            .map(|u| u.path.as_str())
            .collect();
        assert!(paths.contains(&".alternate_layout"));
        assert!(paths.contains(&".default_layout"));

        // Workspace should have 1 config usage
        assert!(nav.usages.contains_key("Workspace"));
        let workspace_usages = &nav.usages["Workspace"];
        assert_eq!(workspace_usages.config_usages.len(), 1);
        assert_eq!(workspace_usages.config_usages[0].path, ".workspaces[]");
    }

    #[test]
    fn test_config_usages_anchor_format() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MyType": { "type": "string" }
            },
            "properties": {
                "my_field": { "$ref": "#/$defs/MyType" }
            }
        });

        let nav = build_navigation(&schema);

        let usages = &nav.usages["MyType"];
        assert_eq!(usages.config_usages.len(), 1);
        // Anchor should NOT have leading dash
        assert_eq!(usages.config_usages[0].anchor, "my_field");
        // Path should have leading dot
        assert_eq!(usages.config_usages[0].path, ".my_field");
    }

    #[test]
    fn test_config_usages_jq_path_format() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Config": { "type": "object" }
            },
            "properties": {
                "monitors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "workspaces": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "config": { "$ref": "#/$defs/Config" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let nav = build_navigation(&schema);

        assert!(nav.usages.contains_key("Config"));
        let config_usages = &nav.usages["Config"];
        assert_eq!(config_usages.config_usages.len(), 1);
        // Should have full jq-style path
        assert_eq!(
            config_usages.config_usages[0].path,
            ".monitors[].workspaces[].config"
        );
        // Anchor should match template ID format (no [] brackets, dots become dashes)
        assert_eq!(
            config_usages.config_usages[0].anchor,
            "monitors-workspaces-config"
        );
    }

    #[test]
    fn test_type_references_collected() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MatchingStrategy": {
                    "type": "string",
                    "enum": ["Equals", "Contains"]
                },
                "IdWithIdentifier": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "matching_strategy": {
                            "anyOf": [
                                { "$ref": "#/$defs/MatchingStrategy" },
                                { "type": "null" }
                            ]
                        }
                    }
                }
            },
            "properties": {
                "rules": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/IdWithIdentifier" }
                }
            }
        });

        let nav = build_navigation(&schema);

        // MatchingStrategy now has transitive config usages (through IdWithIdentifier)
        let matching_strategy = &nav.usages["MatchingStrategy"];
        assert_eq!(matching_strategy.config_usages.len(), 1);
        assert_eq!(
            matching_strategy.config_usages[0].path,
            ".rules[].matching_strategy"
        );

        // It should also have a type reference from IdWithIdentifier
        assert_eq!(matching_strategy.type_references.len(), 1);
        assert_eq!(
            matching_strategy.type_references[0].type_name,
            "IdWithIdentifier"
        );
        assert_eq!(
            matching_strategy.type_references[0].anchor,
            "idwithidentifier"
        );

        // IdWithIdentifier has config usages
        let id_with_identifier = &nav.usages["IdWithIdentifier"];
        assert_eq!(id_with_identifier.config_usages.len(), 1);
        assert_eq!(id_with_identifier.config_usages[0].path, ".rules[]");
    }

    #[test]
    fn test_type_with_both_usages() {
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Colour": {
                    "type": "object",
                    "properties": {
                        "r": { "type": "integer" },
                        "g": { "type": "integer" },
                        "b": { "type": "integer" }
                    }
                },
                "Theme": {
                    "type": "object",
                    "properties": {
                        "primary": { "$ref": "#/$defs/Colour" },
                        "secondary": { "$ref": "#/$defs/Colour" }
                    }
                }
            },
            "properties": {
                "background": { "$ref": "#/$defs/Colour" },
                "theme": { "$ref": "#/$defs/Theme" }
            }
        });

        let nav = build_navigation(&schema);

        // Colour has config usages: direct (background) and transitive through Theme
        let colour = &nav.usages["Colour"];
        assert_eq!(colour.config_usages.len(), 3);

        // Find usages by path and verify anchors
        let background_usage = colour
            .config_usages
            .iter()
            .find(|u| u.path == ".background")
            .expect("should have .background usage");
        // Direct usage at config root -> anchor is "background"
        assert_eq!(background_usage.anchor, "background");

        let primary_usage = colour
            .config_usages
            .iter()
            .find(|u| u.path == ".theme.primary")
            .expect("should have .theme.primary usage");
        // Transitive through Theme -> anchor points to Theme's property: "theme-primary"
        assert_eq!(primary_usage.anchor, "theme-primary");

        let secondary_usage = colour
            .config_usages
            .iter()
            .find(|u| u.path == ".theme.secondary")
            .expect("should have .theme.secondary usage");
        // Transitive through Theme -> anchor points to Theme's property: "theme-secondary"
        assert_eq!(secondary_usage.anchor, "theme-secondary");

        // Also has type reference from Theme
        assert_eq!(colour.type_references.len(), 1);
        assert_eq!(colour.type_references[0].type_name, "Theme");
    }

    #[test]
    fn test_anchor_links_match_property_ids() {
        // Verify that anchors generated for config usages match the IDs generated by the template
        // Template uses: id="{{ path | lower | replace(from=".", to="-") }}"
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Type1": { "type": "string" },
                "Type2": { "type": "string" }
            },
            "properties": {
                "simple_field": { "$ref": "#/$defs/Type1" },
                "nested": {
                    "type": "object",
                    "properties": {
                        "inner_field": { "$ref": "#/$defs/Type2" }
                    }
                }
            }
        });

        let nav = build_navigation(&schema);

        // simple_field: path=simple_field, template ID=simple_field
        let type1 = &nav.usages["Type1"];
        assert_eq!(type1.config_usages[0].anchor, "simple_field");

        // nested.inner_field: path=nested.inner_field, template ID=nested-inner_field
        let type2 = &nav.usages["Type2"];
        assert_eq!(type2.config_usages[0].anchor, "nested-inner_field");
    }

    #[test]
    fn test_transitive_config_usages() {
        // Types nested inside other types should show their full Configuration path
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MonitorConfig": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "workspaces": {
                            "type": "array",
                            "items": { "$ref": "#/$defs/WorkspaceConfig" }
                        }
                    }
                },
                "WorkspaceConfig": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "layout": { "$ref": "#/$defs/Layout" }
                    }
                },
                "Layout": {
                    "type": "string",
                    "enum": ["horizontal", "vertical"]
                }
            },
            "properties": {
                "monitors": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/MonitorConfig" }
                }
            }
        });

        let nav = build_navigation(&schema);

        // MonitorConfig should have direct usage at config root
        // Anchor points to "monitors" in the Configuration section (no [] in IDs)
        let monitor_usages = &nav.usages["MonitorConfig"];
        assert_eq!(monitor_usages.config_usages.len(), 1);
        assert_eq!(monitor_usages.config_usages[0].path, ".monitors[]");
        assert_eq!(monitor_usages.config_usages[0].anchor, "monitors");

        // WorkspaceConfig should have transitive usage through MonitorConfig
        // Anchor points to "monitorconfig-workspaces" (property in MonitorConfig type definition)
        let workspace_usages = &nav.usages["WorkspaceConfig"];
        assert_eq!(workspace_usages.config_usages.len(), 1);
        assert_eq!(
            workspace_usages.config_usages[0].path,
            ".monitors[].workspaces[]"
        );
        assert_eq!(
            workspace_usages.config_usages[0].anchor,
            "monitorconfig-workspaces"
        );

        // Layout should have transitive usage through WorkspaceConfig
        // Anchor points to "workspaceconfig-layout" (property in WorkspaceConfig type definition)
        let layout_usages = &nav.usages["Layout"];
        assert_eq!(layout_usages.config_usages.len(), 1);
        assert_eq!(
            layout_usages.config_usages[0].path,
            ".monitors[].workspaces[].layout"
        );
        assert_eq!(
            layout_usages.config_usages[0].anchor,
            "workspaceconfig-layout"
        );
    }

    #[test]
    fn test_recursive_type_no_infinite_loop() {
        // Recursive types should not cause infinite loops
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Node": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" },
                        "children": {
                            "type": "array",
                            "items": { "$ref": "#/$defs/Node" }
                        }
                    }
                }
            },
            "properties": {
                "root": { "$ref": "#/$defs/Node" }
            }
        });

        let nav = build_navigation(&schema);

        // Should complete without hanging
        let node_usages = &nav.usages["Node"];
        // Node should have at least the direct usage at .root
        assert!(!node_usages.config_usages.is_empty());
        assert!(node_usages.config_usages.iter().any(|u| u.path == ".root"));
    }

    #[test]
    fn test_same_type_multiple_paths() {
        // A type used in multiple places should show all paths
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Colour": {
                    "type": "object",
                    "properties": {
                        "r": { "type": "integer" },
                        "g": { "type": "integer" },
                        "b": { "type": "integer" }
                    }
                },
                "Theme": {
                    "type": "object",
                    "properties": {
                        "primary": { "$ref": "#/$defs/Colour" },
                        "secondary": { "$ref": "#/$defs/Colour" }
                    }
                }
            },
            "properties": {
                "background": { "$ref": "#/$defs/Colour" },
                "foreground": { "$ref": "#/$defs/Colour" },
                "theme": { "$ref": "#/$defs/Theme" }
            }
        });

        let nav = build_navigation(&schema);

        // Colour should have 4 config usages:
        // - .background (direct)
        // - .foreground (direct)
        // - .theme.primary (transitive through Theme)
        // - .theme.secondary (transitive through Theme)
        let colour_usages = &nav.usages["Colour"];
        assert_eq!(colour_usages.config_usages.len(), 4);

        let paths: Vec<&str> = colour_usages
            .config_usages
            .iter()
            .map(|u| u.path.as_str())
            .collect();
        assert!(paths.contains(&".background"));
        assert!(paths.contains(&".foreground"));
        assert!(paths.contains(&".theme.primary"));
        assert!(paths.contains(&".theme.secondary"));
    }

    #[test]
    fn test_oneof_with_object_variants_anchor() {
        // oneOf types where variants have properties should include variant index in anchor
        // This matches template behavior: KomorebiTheme.oneOf[0].bar_accent -> komorebitheme-oneof[0]-bar_accent
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Colour": { "type": "string" },
                "Theme": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "primary": { "$ref": "#/$defs/Colour" },
                                "name": { "type": "string", "const": "light" }
                            }
                        },
                        {
                            "type": "object",
                            "properties": {
                                "primary": { "$ref": "#/$defs/Colour" },
                                "name": { "type": "string", "const": "dark" }
                            }
                        }
                    ]
                }
            },
            "properties": {
                "theme": { "$ref": "#/$defs/Theme" }
            }
        });

        let nav = build_navigation(&schema);

        // Colour should have 2 usages - one from each oneOf variant
        let colour_usages = &nav.usages["Colour"];
        assert_eq!(colour_usages.config_usages.len(), 2);

        // Both should have oneOf[N] in the anchor since variants have properties
        let anchors: Vec<&str> = colour_usages
            .config_usages
            .iter()
            .map(|u| u.anchor.as_str())
            .collect();
        assert!(anchors.contains(&"theme-oneof[0]-primary"));
        assert!(anchors.contains(&"theme-oneof[1]-primary"));
    }

    #[test]
    fn test_anyof_type_union_anchor() {
        // anyOf types that are pure type unions (no properties in variants) should use type anchor
        // e.g., Colour = Hex | Rgb - anchor should be "colour", not "colour-anyof[0]"
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Hex": { "type": "string", "format": "hex" },
                "Rgb": {
                    "type": "object",
                    "properties": {
                        "r": { "type": "integer" },
                        "g": { "type": "integer" },
                        "b": { "type": "integer" }
                    }
                },
                "Colour": {
                    "anyOf": [
                        { "$ref": "#/$defs/Hex" },
                        { "$ref": "#/$defs/Rgb" }
                    ]
                }
            },
            "properties": {
                "background": { "$ref": "#/$defs/Colour" }
            }
        });

        let nav = build_navigation(&schema);

        // Hex and Rgb should have usages through Colour
        // Since Colour's anyOf variants are just $refs (no properties), anchor should be "colour"
        let hex_usages = &nav.usages["Hex"];
        assert_eq!(hex_usages.config_usages.len(), 1);
        assert_eq!(hex_usages.config_usages[0].path, ".background");
        assert_eq!(hex_usages.config_usages[0].anchor, "colour");

        let rgb_usages = &nav.usages["Rgb"];
        assert_eq!(rgb_usages.config_usages.len(), 1);
        assert_eq!(rgb_usages.config_usages[0].path, ".background");
        assert_eq!(rgb_usages.config_usages[0].anchor, "colour");
    }

    #[test]
    fn test_nested_oneof_in_property_no_variant_index() {
        // Properties with anyOf (like optional values) should NOT add variant index
        // e.g., MonitorConfig.wallpaper which is anyOf[Wallpaper, null] -> anchor is "monitorconfig-wallpaper"
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Wallpaper": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                },
                "MonitorConfig": {
                    "type": "object",
                    "properties": {
                        "wallpaper": {
                            "anyOf": [
                                { "$ref": "#/$defs/Wallpaper" },
                                { "type": "null" }
                            ]
                        }
                    }
                }
            },
            "properties": {
                "monitor": { "$ref": "#/$defs/MonitorConfig" }
            }
        });

        let nav = build_navigation(&schema);

        // Wallpaper should have usage through MonitorConfig.wallpaper
        // The anyOf is within a property, so NO variant index in anchor
        let wallpaper_usages = &nav.usages["Wallpaper"];
        assert_eq!(wallpaper_usages.config_usages.len(), 1);
        assert_eq!(wallpaper_usages.config_usages[0].path, ".monitor.wallpaper");
        assert_eq!(
            wallpaper_usages.config_usages[0].anchor,
            "monitorconfig-wallpaper"
        );
    }

    #[test]
    fn test_array_path_strips_brackets_in_anchor() {
        // Array paths like .monitors[] should have anchor "monitors" (no brackets)
        // Template IDs don't include [] notation
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Item": { "type": "string" }
            },
            "properties": {
                "items": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/Item" }
                }
            }
        });

        let nav = build_navigation(&schema);

        let item_usages = &nav.usages["Item"];
        assert_eq!(item_usages.config_usages.len(), 1);
        assert_eq!(item_usages.config_usages[0].path, ".items[]");
        // Anchor should NOT have brackets
        assert_eq!(item_usages.config_usages[0].anchor, "items");
    }

    // ==================== Wrapper Type Detection Tests ====================

    #[test]
    fn test_wrapper_type_detection_with_ref() {
        // Wrapper type pattern: anyOf with object+additionalProperties and direct $ref
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "additionalProperties": { "$ref": "#/$defs/InnerType" }
                },
                { "$ref": "#/$defs/InnerType" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert_eq!(
            prepared.get("_wrapper_type"),
            Some(&Value::Bool(true)),
            "Should detect wrapper type pattern"
        );
        assert!(
            prepared.get("_wrapper_inner").is_some(),
            "Should have _wrapper_inner"
        );
        assert_eq!(
            prepared.get("_wrapper_inner").unwrap().get("$ref"),
            Some(&Value::String("#/$defs/InnerType".to_string())),
            "Inner type should be the $ref"
        );
    }

    #[test]
    fn test_wrapper_type_detection_with_primitive() {
        // Wrapper type with primitive inner type
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "additionalProperties": { "type": "boolean" }
                },
                { "type": "boolean" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert_eq!(
            prepared.get("_wrapper_type"),
            Some(&Value::Bool(true)),
            "Should detect wrapper type with primitive"
        );
        assert_eq!(
            prepared
                .get("_wrapper_inner")
                .unwrap()
                .get("type")
                .unwrap()
                .as_str(),
            Some("boolean"),
            "Inner type should be boolean"
        );
    }

    #[test]
    fn test_wrapper_type_detection_reversed_order() {
        // Wrapper type with map variant second
        let schema = json!({
            "anyOf": [
                { "type": "integer" },
                {
                    "type": "object",
                    "additionalProperties": { "type": "integer" }
                }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert_eq!(
            prepared.get("_wrapper_type"),
            Some(&Value::Bool(true)),
            "Should detect wrapper type regardless of variant order"
        );
    }

    #[test]
    fn test_non_wrapper_mismatched_types() {
        // Not a wrapper: inner types don't match
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                { "type": "integer" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert!(
            prepared.get("_wrapper_type").is_none(),
            "Should NOT detect wrapper when inner types don't match"
        );
    }

    #[test]
    fn test_option_not_detected_as_wrapper() {
        // Option<T> pattern (anyOf with null) should NOT be detected as wrapper
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                { "type": "null" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert!(
            prepared.get("_wrapper_type").is_none(),
            "Option<Map<K,V>> should NOT be detected as wrapper type"
        );
    }

    #[test]
    fn test_regular_anyof_not_wrapper() {
        // Regular anyOf (no additionalProperties pattern)
        let schema = json!({
            "anyOf": [
                { "type": "string" },
                { "type": "integer" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert!(
            prepared.get("_wrapper_type").is_none(),
            "Regular anyOf should NOT be detected as wrapper"
        );
    }

    #[test]
    fn test_wrapper_detection_nested() {
        // Wrapper type nested inside a larger schema
        let schema = json!({
            "$defs": {
                "WrapperType": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "$ref": "#/$defs/Inner" }
                        },
                        { "$ref": "#/$defs/Inner" }
                    ]
                },
                "Inner": { "type": "string" }
            }
        });

        let prepared = prepare_for_template(&schema);

        let wrapper_def = prepared.get("$defs").unwrap().get("WrapperType").unwrap();
        assert_eq!(
            wrapper_def.get("_wrapper_type"),
            Some(&Value::Bool(true)),
            "Should detect nested wrapper type"
        );
    }

    #[test]
    fn test_wrapper_with_integer_constraints() {
        // Wrapper type where inner type has constraints (should still match by type)
        let schema = json!({
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
        });

        let prepared = prepare_for_template(&schema);

        assert_eq!(
            prepared.get("_wrapper_type"),
            Some(&Value::Bool(true)),
            "Should detect wrapper even with constraints on inner type"
        );
    }

    #[test]
    fn test_three_variant_anyof_not_wrapper() {
        // anyOf with 3+ variants should NOT be detected as wrapper
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                { "type": "string" },
                { "type": "null" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert!(
            prepared.get("_wrapper_type").is_none(),
            "anyOf with 3+ variants should NOT be detected as wrapper"
        );
    }

    #[test]
    fn test_object_without_additional_properties_not_wrapper() {
        // Object without additionalProperties should NOT be detected as wrapper
        let schema = json!({
            "anyOf": [
                {
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                },
                { "type": "string" }
            ]
        });

        let prepared = prepare_for_template(&schema);

        assert!(
            prepared.get("_wrapper_type").is_none(),
            "Object with properties (not additionalProperties) should NOT be wrapper"
        );
    }

    #[test]
    fn test_type_names_ending_in_digits_not_stripped() {
        // Legitimate type names ending in digits should NOT have _display_name added
        let schema = json!({
            "$defs": {
                "Base16": { "type": "string" },
                "Sha256": { "type": "string" },
                "Level3": { "type": "object" }
            }
        });

        let prepared = prepare_for_template(&schema);
        let defs = prepared.get("$defs").unwrap().as_object().unwrap();

        // Type names that legitimately end in digits should NOT have _display_name added
        assert!(
            defs.get("Base16").unwrap().get("_display_name").is_none(),
            "Base16 should not have _display_name"
        );
        assert!(
            defs.get("Sha256").unwrap().get("_display_name").is_none(),
            "Sha256 should not have _display_name"
        );
        assert!(
            defs.get("Level3").unwrap().get("_display_name").is_none(),
            "Level3 should not have _display_name"
        );
    }

    #[test]
    fn test_wrapper_types_get_display_name_stripped() {
        // Real-world pattern from komorebi: PerAnimationPrefixConfig<T> generates
        // multiple $defs entries for different type parameters:
        // - PerAnimationPrefixConfig (bool)
        // - PerAnimationPrefixConfig2 (uint64)
        // - PerAnimationPrefixConfig3 (AnimationStyle ref)
        // All should display as "PerAnimationPrefixConfig"
        let schema = json!({
            "$defs": {
                "AnimationStyle": { "type": "string" },
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
                },
                "PerAnimationPrefixConfig3": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "$ref": "#/$defs/AnimationStyle" }
                        },
                        { "$ref": "#/$defs/AnimationStyle" }
                    ]
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let defs = prepared.get("$defs").unwrap().as_object().unwrap();

        // All three should be detected as wrapper types
        assert_eq!(
            defs.get("PerAnimationPrefixConfig")
                .unwrap()
                .get("_wrapper_type"),
            Some(&json!(true)),
            "PerAnimationPrefixConfig should be detected as wrapper type"
        );
        assert_eq!(
            defs.get("PerAnimationPrefixConfig2")
                .unwrap()
                .get("_wrapper_type"),
            Some(&json!(true)),
            "PerAnimationPrefixConfig2 should be detected as wrapper type"
        );
        assert_eq!(
            defs.get("PerAnimationPrefixConfig3")
                .unwrap()
                .get("_wrapper_type"),
            Some(&json!(true)),
            "PerAnimationPrefixConfig3 should be detected as wrapper type"
        );

        // The base type has no digits, so no _display_name needed
        assert!(
            defs.get("PerAnimationPrefixConfig")
                .unwrap()
                .get("_display_name")
                .is_none(),
            "Base wrapper type should not have _display_name"
        );

        // Numbered variants should have display name with digits stripped
        assert_eq!(
            defs.get("PerAnimationPrefixConfig2")
                .unwrap()
                .get("_display_name"),
            Some(&json!("PerAnimationPrefixConfig")),
            "PerAnimationPrefixConfig2 should have digits stripped"
        );
        assert_eq!(
            defs.get("PerAnimationPrefixConfig3")
                .unwrap()
                .get("_display_name"),
            Some(&json!("PerAnimationPrefixConfig")),
            "PerAnimationPrefixConfig3 should have digits stripped"
        );
    }

    #[test]
    fn test_default_with_direct_ref_gets_annotated() {
        // When a property has a default and a direct $ref, it should get _default_ref
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "Layout": {
                    "type": "string",
                    "enum": ["horizontal", "vertical"]
                }
            },
            "properties": {
                "layout": {
                    "$ref": "#/$defs/Layout",
                    "default": "horizontal"
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let props = prepared.get("properties").unwrap().as_object().unwrap();
        let layout_prop = props.get("layout").unwrap();

        assert_eq!(
            layout_prop.get("_default_ref"),
            Some(&json!("Layout")),
            "Direct $ref with default should have _default_ref"
        );
    }

    #[test]
    fn test_default_with_anyof_option_pattern_gets_annotated() {
        // When a property has a default and anyOf Option<T> pattern, it should get _default_ref
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "MoveBehaviour": {
                    "oneOf": [
                        { "type": "string", "const": "Swap" },
                        { "type": "string", "const": "Insert" }
                    ]
                }
            },
            "properties": {
                "move_behaviour": {
                    "anyOf": [
                        { "$ref": "#/$defs/MoveBehaviour" },
                        { "type": "null" }
                    ],
                    "default": "Swap"
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let props = prepared.get("properties").unwrap().as_object().unwrap();
        let move_prop = props.get("move_behaviour").unwrap();

        assert_eq!(
            move_prop.get("_default_ref"),
            Some(&json!("MoveBehaviour")),
            "anyOf Option<T> pattern with default should have _default_ref"
        );
    }

    #[test]
    fn test_default_with_oneof_option_pattern_gets_annotated() {
        // When a property has a default and oneOf Option<T> pattern, it should get _default_ref
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "BorderStyle": {
                    "type": "string",
                    "enum": ["solid", "dashed"]
                }
            },
            "properties": {
                "border_style": {
                    "oneOf": [
                        { "$ref": "#/$defs/BorderStyle" },
                        { "type": "null" }
                    ],
                    "default": "solid"
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let props = prepared.get("properties").unwrap().as_object().unwrap();
        let border_prop = props.get("border_style").unwrap();

        assert_eq!(
            border_prop.get("_default_ref"),
            Some(&json!("BorderStyle")),
            "oneOf Option<T> pattern with default should have _default_ref"
        );
    }

    #[test]
    fn test_primitive_default_not_annotated() {
        // Primitive defaults (boolean, integer, etc.) should NOT get _default_ref
        let schema = json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "enabled": {
                    "type": ["boolean", "null"],
                    "default": true
                },
                "count": {
                    "type": ["integer", "null"],
                    "default": 10
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let props = prepared.get("properties").unwrap().as_object().unwrap();

        assert!(
            props.get("enabled").unwrap().get("_default_ref").is_none(),
            "Boolean default should not have _default_ref"
        );
        assert!(
            props.get("count").unwrap().get("_default_ref").is_none(),
            "Integer default should not have _default_ref"
        );
    }

    #[test]
    fn test_default_without_ref_not_annotated() {
        // Defaults on types without $ref should NOT get _default_ref
        let schema = json!({
            "title": "Test",
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "default": "untitled"
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let props = prepared.get("properties").unwrap().as_object().unwrap();

        assert!(
            props.get("name").unwrap().get("_default_ref").is_none(),
            "String type without $ref should not have _default_ref"
        );
    }

    #[test]
    fn test_nested_defaults_annotated() {
        // Defaults inside nested structures should also be annotated
        let schema = json!({
            "title": "Test",
            "type": "object",
            "$defs": {
                "AnimationStyle": {
                    "type": "string",
                    "enum": ["Linear", "EaseIn", "EaseOut"]
                },
                "AnimationConfig": {
                    "type": "object",
                    "properties": {
                        "style": {
                            "anyOf": [
                                { "$ref": "#/$defs/AnimationStyle" },
                                { "type": "null" }
                            ],
                            "default": "Linear"
                        }
                    }
                }
            },
            "properties": {
                "animation": { "$ref": "#/$defs/AnimationConfig" }
            }
        });

        let prepared = prepare_for_template(&schema);
        let defs = prepared.get("$defs").unwrap().as_object().unwrap();
        let anim_config = defs.get("AnimationConfig").unwrap();
        let style_prop = anim_config.get("properties").unwrap().get("style").unwrap();

        assert_eq!(
            style_prop.get("_default_ref"),
            Some(&json!("AnimationStyle")),
            "Nested default with $ref should have _default_ref"
        );
    }

    // =========================================================================
    // Schemars 1.2 Enum-Keyed Map Tests
    // =========================================================================

    #[test]
    fn test_schemars_1_2_enum_keyed_map_wrapper_detection() {
        // Test that enum-keyed maps (schemars 1.2 pattern) are detected as wrapper types
        // This pattern is generated for HashMap<EnumKey, T>
        let schema = json!({
            "$defs": {
                "PerAnimationPrefixConfig": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "movement": { "type": "boolean" }
                            },
                            "additionalProperties": false
                        },
                        { "type": "boolean" }
                    ]
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let def = &prepared["$defs"]["PerAnimationPrefixConfig"];

        assert_eq!(
            def["_wrapper_type"],
            json!(true),
            "Enum-keyed map should be detected as wrapper type"
        );
        assert_eq!(
            def["_wrapper_inner"]["type"],
            json!("boolean"),
            "Wrapper inner type should be boolean"
        );
    }

    #[test]
    fn test_schemars_1_2_enum_keyed_map_with_ref() {
        // Test enum-keyed map where the value is a $ref
        let schema = json!({
            "$defs": {
                "AnimationStyle": {
                    "type": "string",
                    "enum": ["Linear", "EaseIn"]
                },
                "PerAnimationPrefixConfig": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "movement": { "$ref": "#/$defs/AnimationStyle" }
                            },
                            "additionalProperties": false
                        },
                        { "$ref": "#/$defs/AnimationStyle" }
                    ]
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let def = &prepared["$defs"]["PerAnimationPrefixConfig"];

        assert_eq!(
            def["_wrapper_type"],
            json!(true),
            "Enum-keyed map with $ref should be detected as wrapper type"
        );
        assert_eq!(
            def["_wrapper_inner"]["$ref"],
            json!("#/$defs/AnimationStyle"),
            "Wrapper inner type should be the $ref"
        );
    }

    #[test]
    fn test_enum_keyed_map_usage_links_to_parent() {
        // Test that usage links for enum-keyed map properties don't include the enum key name
        // (since those properties are rendered inline without heading anchors)
        let schema = json!({
            "type": "object",
            "$defs": {
                "Config": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "variant_a": { "$ref": "#/$defs/Inner" }
                            },
                            "additionalProperties": false
                        },
                        { "$ref": "#/$defs/Inner" }
                    ]
                },
                "Inner": { "type": "string" }
            },
            "properties": {
                "config": { "$ref": "#/$defs/Config" }
            }
        });

        let prepared = prepare_for_template(&schema);
        let nav = build_navigation(&prepared);
        let inner_usages = nav.usages.get("Inner").unwrap();

        // Should not have links containing "variant_a" (the enum key)
        for usage in &inner_usages.config_usages {
            assert!(
                !usage.anchor.contains("variant_a"),
                "Usage anchor should not contain enum key 'variant_a': {}",
                usage.anchor
            );
        }
    }

    #[test]
    fn test_is_enum_keyed_map() {
        // Test the is_enum_keyed_map helper function
        let enum_keyed_map = json!({
            "type": "object",
            "properties": {
                "movement": { "type": "boolean" }
            },
            "additionalProperties": false
        });

        assert!(
            is_enum_keyed_map(&enum_keyed_map),
            "Should detect enum-keyed map pattern"
        );

        // Regular object with properties should NOT be detected
        let regular_object = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        assert!(
            !is_enum_keyed_map(&regular_object),
            "Regular object without additionalProperties: false should not be detected"
        );

        // Object with additionalProperties as schema should NOT be detected
        let map_with_additional = json!({
            "type": "object",
            "additionalProperties": { "type": "string" }
        });

        assert!(
            !is_enum_keyed_map(&map_with_additional),
            "Map with additionalProperties schema should not be detected as enum-keyed"
        );
    }

    #[test]
    fn test_schemars_1_1_wrapper_still_works() {
        // Ensure backward compatibility: schemars 1.1 pattern should still be detected
        let schema = json!({
            "$defs": {
                "WrapperType": {
                    "anyOf": [
                        {
                            "type": "object",
                            "additionalProperties": { "type": "boolean" }
                        },
                        { "type": "boolean" }
                    ]
                }
            }
        });

        let prepared = prepare_for_template(&schema);
        let def = &prepared["$defs"]["WrapperType"];

        assert_eq!(
            def["_wrapper_type"],
            json!(true),
            "Schemars 1.1 wrapper pattern should still be detected"
        );
        assert_eq!(
            def["_wrapper_inner"]["type"],
            json!("boolean"),
            "Wrapper inner type should be boolean"
        );
    }
}
