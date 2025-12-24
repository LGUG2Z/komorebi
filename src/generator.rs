use crate::format::json_to_nix_literal;
use crate::format::module_header;
use crate::format::options_block_close;
use crate::format::options_block_open;
use crate::prepare::collect_definitions;
use crate::prepare::detect_const_enum;
use crate::prepare::detect_externally_tagged_union;
use crate::prepare::detect_internally_tagged_union;
use crate::prepare::detect_mixed_enum_object_union;
use crate::prepare::detect_nullable_anyof;
use crate::prepare::detect_simple_enum;
use crate::prepare::get_additional_properties;
use crate::prepare::get_array_items;
use crate::prepare::get_default;
use crate::prepare::get_description;
use crate::prepare::get_pattern_properties;
use crate::prepare::get_prefix_items_type;
use crate::prepare::get_properties;
use crate::prepare::get_ref;
use crate::prepare::get_schema_type;
use crate::prepare::resolve_ref_name;
use crate::types::NixOption;
use crate::types::NixType;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;

/// Configuration options for the generator
#[derive(Debug, Clone)]
pub struct GeneratorOptions {
    /// The root option name (e.g., "komorebi" or "komorebi-bar")
    pub root_name: String,
    /// Description for the root option
    pub root_description: String,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self {
            root_name: "config".to_string(),
            root_description: "Configuration options".to_string(),
        }
    }
}

/// Error type for generation failures
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("Invalid schema: {0}")]
    InvalidSchema(String),
    #[error("Unsupported schema construct: {0}")]
    UnsupportedConstruct(String),
    #[error("Unresolved reference: {0}")]
    UnresolvedRef(String),
}

/// The main Nix code generator
pub struct Generator {
    definitions: HashMap<String, Value>,
    /// Cache for resolved type definitions to avoid infinite recursion
    resolving: Vec<String>,
    /// Count of how many times each $def is referenced
    ref_counts: HashMap<String, usize>,
    /// Pre-generated types for frequently used definitions
    type_bindings: HashMap<String, NixType>,
    /// Whether we're in the second pass (using type refs)
    use_type_refs: bool,
}

impl Generator {
    /// Create a new generator from a JSON schema
    pub fn new(schema: &Value) -> Self {
        Self {
            definitions: collect_definitions(schema),
            resolving: Vec::new(),
            ref_counts: HashMap::new(),
            type_bindings: HashMap::new(),
            use_type_refs: false,
        }
    }

    /// Convert a definition name to a valid Nix identifier
    fn def_to_nix_ident(name: &str) -> String {
        // Convert PascalCase to camelCase and handle special chars
        let mut result = String::new();
        let mut chars = name.chars().peekable();
        let mut first = true;

        while let Some(c) = chars.next() {
            if first {
                result.push(c.to_ascii_lowercase());
                first = false;
            } else if c == '-' || c == '_' {
                // Skip separator and capitalize next char
                if let Some(&next) = chars.peek() {
                    chars.next();
                    result.push(next.to_ascii_uppercase());
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    /// First pass: count how many times each $ref is used
    fn count_refs(&mut self, schema: &Value) {
        if let Some(ref_str) = get_ref(schema)
            && let Some(type_name) = resolve_ref_name(ref_str)
        {
            *self.ref_counts.entry(type_name.to_string()).or_insert(0) += 1;
        }

        // Recurse into nested schemas
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            for (_, prop_schema) in props {
                self.count_refs(prop_schema);
            }
        }
        if let Some(items) = schema.get("items") {
            self.count_refs(items);
        }
        if let Some(additional) = schema.get("additionalProperties")
            && additional.is_object()
        {
            self.count_refs(additional);
        }
        if let Some(pattern_props) = schema.get("patternProperties").and_then(|p| p.as_object()) {
            for (_, prop_schema) in pattern_props {
                self.count_refs(prop_schema);
            }
        }
        if let Some(one_of) = schema.get("oneOf").and_then(|o| o.as_array()) {
            for variant in one_of {
                self.count_refs(variant);
            }
        }
        if let Some(any_of) = schema.get("anyOf").and_then(|a| a.as_array()) {
            for variant in any_of {
                self.count_refs(variant);
            }
        }
        if let Some(all_of) = schema.get("allOf").and_then(|a| a.as_array()) {
            for variant in all_of {
                self.count_refs(variant);
            }
        }
    }

    /// Build a dependency graph of $defs
    /// Returns HashMap<def_name, Vec<dependencies>>
    fn build_dependency_graph(&self) -> HashMap<String, Vec<String>> {
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();

        for (name, def) in &self.definitions {
            let deps = Self::collect_refs_in_schema(def);
            graph.insert(name.clone(), deps);
        }

        graph
    }

    /// Collect all $ref names referenced in a schema value
    fn collect_refs_in_schema(schema: &Value) -> Vec<String> {
        let mut refs = Vec::new();
        Self::collect_refs_recursive(schema, &mut refs);
        refs
    }

    /// Recursively collect $ref names from a schema
    fn collect_refs_recursive(schema: &Value, refs: &mut Vec<String>) {
        // Check for direct $ref
        if let Some(ref_str) = get_ref(schema)
            && let Some(type_name) = resolve_ref_name(ref_str)
        {
            refs.push(type_name.to_string());
        }

        // Recurse into properties
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            for (_, prop_schema) in props {
                Self::collect_refs_recursive(prop_schema, refs);
            }
        }

        // Recurse into items
        if let Some(items) = schema.get("items") {
            Self::collect_refs_recursive(items, refs);
        }

        // Recurse into additionalProperties
        if let Some(additional) = schema.get("additionalProperties")
            && additional.is_object()
        {
            Self::collect_refs_recursive(additional, refs);
        }

        // Recurse into patternProperties
        if let Some(pattern_props) = schema.get("patternProperties").and_then(|p| p.as_object()) {
            for (_, prop_schema) in pattern_props {
                Self::collect_refs_recursive(prop_schema, refs);
            }
        }

        // Recurse into oneOf
        if let Some(one_of) = schema.get("oneOf").and_then(|o| o.as_array()) {
            for variant in one_of {
                Self::collect_refs_recursive(variant, refs);
            }
        }

        // Recurse into anyOf
        if let Some(any_of) = schema.get("anyOf").and_then(|a| a.as_array()) {
            for variant in any_of {
                Self::collect_refs_recursive(variant, refs);
            }
        }

        // Recurse into allOf
        if let Some(all_of) = schema.get("allOf").and_then(|a| a.as_array()) {
            for variant in all_of {
                Self::collect_refs_recursive(variant, refs);
            }
        }

        // Recurse into prefixItems
        if let Some(prefix_items) = schema.get("prefixItems").and_then(|p| p.as_array()) {
            for item in prefix_items {
                Self::collect_refs_recursive(item, refs);
            }
        }
    }

    /// Topologically sort definitions so dependencies come before dependents
    fn topological_sort(graph: &HashMap<String, Vec<String>>) -> Vec<String> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new(); // For cycle detection

        for name in graph.keys() {
            Self::topo_visit(name, graph, &mut visited, &mut temp_visited, &mut result);
        }

        result
    }

    /// DFS visit for topological sort
    fn topo_visit(
        name: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) {
        if visited.contains(name) {
            return;
        }
        if temp_visited.contains(name) {
            // Cycle detected - skip (will use Anything for recursive types)
            return;
        }

        temp_visited.insert(name.to_string());

        if let Some(deps) = graph.get(name) {
            for dep in deps {
                // Only visit if this dependency is in our graph (i.e., it's a $def)
                if graph.contains_key(dep) {
                    Self::topo_visit(dep, graph, visited, temp_visited, result);
                }
            }
        }

        temp_visited.remove(name);
        visited.insert(name.to_string());
        result.push(name.to_string());
    }

    /// Generate type bindings for frequently used definitions
    fn generate_type_bindings(&mut self) -> Result<(), GeneratorError> {
        // Build dependency graph
        let graph = self.build_dependency_graph();

        // Topologically sort definitions (dependencies come first)
        let sorted_defs = Self::topological_sort(&graph);

        // Only generate bindings for refs used more than once
        let frequent_refs: HashSet<String> = self
            .ref_counts
            .iter()
            .filter(|&(_, &count)| count > 1)
            .map(|(name, _)| name.clone())
            .collect();

        // Generate bindings in topological order (leaf types first)
        // This allows composite types to reference already-generated leaf types
        self.use_type_refs = true;

        for ref_name in sorted_defs {
            if !frequent_refs.contains(&ref_name) {
                continue;
            }

            if let Some(def) = self.definitions.get(&ref_name).cloned() {
                let nix_type = self.schema_to_nix_type(&def)?;
                self.type_bindings.insert(ref_name.clone(), nix_type);
            }
        }

        Ok(())
    }

    /// Generate the let bindings block
    fn generate_let_block(&self) -> String {
        if self.type_bindings.is_empty() {
            return String::new();
        }

        let mut bindings: Vec<(String, String)> = self
            .type_bindings
            .iter()
            .map(|(name, nix_type)| {
                let ident = Self::def_to_nix_ident(name);
                let expr = nix_type.to_nix_expr();
                (ident, expr)
            })
            .collect();

        // Sort for deterministic output
        bindings.sort_by(|a, b| a.0.cmp(&b.0));

        let mut output = String::from("let\n");
        for (ident, expr) in bindings {
            output.push_str(&format!("  {} = {};\n", ident, expr));
        }
        output.push_str("in\n");
        output
    }

    /// Generate Nix module code from the schema
    pub fn generate(
        &mut self,
        schema: &Value,
        options: &GeneratorOptions,
    ) -> Result<String, GeneratorError> {
        // First pass: count ref usage throughout the schema
        self.count_refs(schema);
        // Also count refs in definitions themselves (they can reference each other)
        let defs: Vec<Value> = self.definitions.values().cloned().collect();
        for def in &defs {
            self.count_refs(def);
        }

        // Generate type bindings for frequently used definitions
        self.generate_type_bindings()?;

        let mut output = String::new();

        // Module header
        output.push_str(module_header());
        output.push('\n');

        // Generate let bindings if any
        let let_block = self.generate_let_block();
        if !let_block.is_empty() {
            output.push_str(&let_block);
        }

        // Open options block
        output.push_str(&options_block_open(&options.root_name));
        output.push('\n');

        // Generate options from properties (second pass with type refs enabled)
        if let Some(properties) = get_properties(schema) {
            for (name, prop_schema) in properties {
                let nix_option = self.schema_to_option(name, prop_schema)?;
                output.push_str(&nix_option.to_nix_option_def(4));
                output.push('\n');
            }
        }

        // Close options block
        output.push_str(&options_block_close(&options.root_description));
        output.push('\n');

        Ok(output)
    }

    /// Convert a JSON schema to a NixOption
    fn schema_to_option(
        &mut self,
        name: &str,
        schema: &Value,
    ) -> Result<NixOption, GeneratorError> {
        let nix_type = self.schema_to_nix_type(schema)?;
        let mut option = NixOption::new(name, nix_type);

        if let Some(desc) = get_description(schema) {
            option.description = Some(desc.to_string());
        }

        if let Some(default) = get_default(schema) {
            option.default = Some(json_to_nix_literal(default));
        }

        Ok(option)
    }

    /// Convert a JSON schema to a NixType
    fn schema_to_nix_type(&mut self, schema: &Value) -> Result<NixType, GeneratorError> {
        // Handle $ref first
        if let Some(ref_str) = get_ref(schema) {
            return self.resolve_ref(ref_str);
        }

        // Check for nullable anyOf pattern: anyOf: [{$ref: T}, {type: null}]
        if let Some(inner) = detect_nullable_anyof(schema) {
            let inner_type = self.schema_to_nix_type(inner)?;
            return Ok(inner_type.make_nullable());
        }

        // Check for nullable type array: ["array", "null"] or ["object", "null"] with extra schema info
        // This needs to be handled BEFORE detect_nullable_type_array to preserve items/properties
        if let Some(type_val) = schema.get("type").and_then(|v| v.as_array())
            && type_val.len() == 2
        {
            let has_null = type_val.iter().any(|t| t.as_str() == Some("null"));
            let other_type = type_val
                .iter()
                .find_map(|t| t.as_str().filter(|&s| s != "null"));

            if has_null && let Some(other) = other_type {
                match other {
                    "array" => {
                        // Handle array with items - e.g., ["array", "null"] with items: {...}
                        let items_type = if let Some(items) = get_array_items(schema) {
                            self.schema_to_nix_type(items)?
                        } else {
                            NixType::Str // Default to string if no items specified
                        };
                        return Ok(NixType::NullOr(Box::new(NixType::ListOf(Box::new(
                            items_type,
                        )))));
                    }
                    "object" => {
                        // Handle object with properties/additionalProperties/patternProperties
                        let inner = self.object_schema_to_nix_type(schema)?;
                        return Ok(inner.make_nullable());
                    }
                    _ => {
                        // Handle primitives like ["string", "null"], ["integer", "null"]
                        let inner_type = self.primitive_to_nix_type(other)?;
                        return Ok(inner_type.make_nullable());
                    }
                }
            }
        }

        // Check for const enum pattern in oneOf
        if let Some(variants) = detect_const_enum(schema) {
            return Ok(NixType::Enum(variants));
        }

        // Check for simple enum
        if let Some(variants) = detect_simple_enum(schema) {
            return Ok(NixType::Enum(variants));
        }

        // Check for internally tagged union (oneOf with shared discriminator field)
        // Example: Grouping with { "kind": "None" }, { "kind": "Bar", ... }
        // For these, we generate a submodule with an enum for the discriminator
        // and optional fields for the variant-specific data
        if let Some((discriminator, variants)) = detect_internally_tagged_union(schema) {
            // Extract the enum values from the discriminator
            let enum_values: Vec<String> = variants.iter().map(|(val, _)| val.clone()).collect();

            // Create a submodule with the discriminator as an enum and other shared fields
            let mut options = vec![NixOption::new(&discriminator, NixType::Enum(enum_values))];

            // Collect all other properties from all variants (excluding the discriminator)
            // Track property types to merge them if they differ across variants
            let mut prop_types: HashMap<String, Vec<NixType>> = HashMap::new();

            for (_, variant_schema) in &variants {
                if let Some(props) = variant_schema.get("properties").and_then(|p| p.as_object()) {
                    for (prop_name, prop_schema) in props {
                        if prop_name != &discriminator {
                            let prop_type = self.schema_to_nix_type(prop_schema)?;
                            prop_types
                                .entry(prop_name.clone())
                                .or_default()
                                .push(prop_type);
                        }
                    }
                }
                // Also handle $ref in variants
                if let Some(ref_str) = variant_schema.get("$ref").and_then(|r| r.as_str())
                    && let Ok(ref_type) = self.resolve_ref(ref_str)
                {
                    // If the ref resolves to a submodule, merge its options
                    if let NixType::Submodule(ref_opts) = ref_type {
                        for opt in ref_opts {
                            prop_types
                                .entry(opt.name.clone())
                                .or_default()
                                .push(opt.nix_type.clone());
                        }
                    }
                }
            }

            // Build options from collected types, merging with OneOf if types differ
            for (prop_name, types) in prop_types {
                // Deduplicate types
                let mut unique_types: Vec<NixType> = Vec::new();
                for t in types {
                    if !unique_types.contains(&t) {
                        unique_types.push(t);
                    }
                }

                let merged_type = if unique_types.len() == 1 {
                    unique_types.into_iter().next().unwrap()
                } else {
                    NixType::OneOf(unique_types)
                };

                options.push(NixOption::new(&prop_name, merged_type.make_nullable()));
            }

            return Ok(NixType::Submodule(options));
        }

        // Check for externally tagged union (oneOf with single-property object variants)
        // This must be checked BEFORE generic oneOf handling
        // Example: WidgetConfig with variants like { "Komorebi": {...} }, { "Battery": {...} }
        if let Some(variants) = detect_externally_tagged_union(schema) {
            let typed_variants: Result<Vec<(String, NixType)>, GeneratorError> = variants
                .into_iter()
                .map(|(tag, inner_schema)| {
                    let inner_type = self.schema_to_nix_type(&inner_schema)?;
                    Ok((tag, inner_type))
                })
                .collect();
            return Ok(NixType::TaggedUnion(typed_variants?));
        }

        // Check for mixed enum + object union (oneOf with string consts AND tagged objects)
        // Example: DateFormat with "Custom" enum value and { "Custom": "..." } object
        if let Some((enum_variants, object_variants)) = detect_mixed_enum_object_union(schema) {
            let mut types = Vec::new();

            // Add enum type for string variants
            if !enum_variants.is_empty() {
                types.push(NixType::Enum(enum_variants));
            }

            // Add tagged union for object variants
            if !object_variants.is_empty() {
                let typed_variants: Result<Vec<(String, NixType)>, GeneratorError> =
                    object_variants
                        .into_iter()
                        .map(|(tag, inner_schema)| {
                            let inner_type = self.schema_to_nix_type(&inner_schema)?;
                            Ok((tag, inner_type))
                        })
                        .collect();
                types.push(NixType::TaggedUnion(typed_variants?));
            }

            return if types.len() == 1 {
                Ok(types.into_iter().next().unwrap())
            } else {
                Ok(NixType::OneOf(types))
            };
        }

        // Check for oneOf with complex variants
        if let Some(one_of) = schema.get("oneOf").and_then(|v| v.as_array()) {
            let types: Result<Vec<NixType>, _> = one_of
                .iter()
                .map(|variant| self.schema_to_nix_type(variant))
                .collect();
            return Ok(NixType::OneOf(types?));
        }

        // Check for anyOf without null (union types) - should become oneOf
        if let Some(any_of) = schema.get("anyOf").and_then(|v| v.as_array()) {
            // Only process if this wasn't already handled as nullable pattern
            let has_null = any_of.iter().any(|v| {
                v.get("type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|s| s == "null")
            });
            if !has_null {
                let types: Result<Vec<NixType>, _> = any_of
                    .iter()
                    .map(|variant| self.schema_to_nix_type(variant))
                    .collect();
                return Ok(NixType::OneOf(types?));
            }
        }

        // Handle by primary type
        if let Some(type_str) = get_schema_type(schema) {
            return self.typed_schema_to_nix_type(type_str, schema);
        }

        // If we have properties but no type, treat as object
        if get_properties(schema).is_some() {
            return self.typed_schema_to_nix_type("object", schema);
        }

        // Fallback to anything
        Ok(NixType::Anything)
    }

    /// Convert a typed schema to NixType
    fn typed_schema_to_nix_type(
        &mut self,
        type_str: &str,
        schema: &Value,
    ) -> Result<NixType, GeneratorError> {
        match type_str {
            "string" => Ok(NixType::Str),
            "integer" => Ok(NixType::Int),
            "boolean" => Ok(NixType::Bool),
            "number" => Ok(NixType::Float),
            "null" => Ok(NixType::NullOr(Box::new(NixType::Anything))),
            "array" => {
                let items_type = if let Some(items) = get_array_items(schema) {
                    self.schema_to_nix_type(items)?
                } else if let Some(prefix_item) = get_prefix_items_type(schema) {
                    // Handle tuple types (prefixItems) - use the first item type
                    self.schema_to_nix_type(prefix_item)?
                } else {
                    NixType::Str // Default to str if no items specified
                };
                Ok(NixType::ListOf(Box::new(items_type)))
            }
            "object" => self.object_schema_to_nix_type(schema),
            _ => Err(GeneratorError::UnsupportedConstruct(format!(
                "unknown type: {}",
                type_str
            ))),
        }
    }

    /// Convert an object schema to NixType
    fn object_schema_to_nix_type(&mut self, schema: &Value) -> Result<NixType, GeneratorError> {
        // Check for additionalProperties (map type)
        if let Some(ap_schema) = get_additional_properties(schema) {
            let value_type = self.schema_to_nix_type(ap_schema)?;
            return Ok(NixType::AttrsOf(Box::new(value_type)));
        }

        // Check for patternProperties (also map type)
        if let Some(pp_schema) = get_pattern_properties(schema) {
            let value_type = self.schema_to_nix_type(pp_schema)?;
            return Ok(NixType::AttrsOf(Box::new(value_type)));
        }

        // Handle object with properties as submodule
        if let Some(properties) = get_properties(schema) {
            let mut options = Vec::new();
            for (name, prop_schema) in properties {
                let option = self.schema_to_option(name, prop_schema)?;
                options.push(option);
            }
            return Ok(NixType::Submodule(options));
        }

        // Empty object or object with no constraints
        Ok(NixType::AttrsOf(Box::new(NixType::Anything)))
    }

    /// Resolve a $ref to a NixType
    fn resolve_ref(&mut self, ref_str: &str) -> Result<NixType, GeneratorError> {
        let type_name = resolve_ref_name(ref_str).ok_or_else(|| {
            GeneratorError::UnresolvedRef(format!("invalid ref format: {}", ref_str))
        })?;

        // If we're using type refs and this type has a binding, return a TypeRef
        if self.use_type_refs && self.type_bindings.contains_key(type_name) {
            return Ok(NixType::TypeRef(Self::def_to_nix_ident(type_name)));
        }

        // Check for recursion
        if self.resolving.contains(&type_name.to_string()) {
            // For recursive types, return Anything to break the cycle
            return Ok(NixType::Anything);
        }

        // Get the definition
        let def = self.definitions.get(type_name).cloned().ok_or_else(|| {
            GeneratorError::UnresolvedRef(format!("definition not found: {}", type_name))
        })?;

        // Mark as resolving to detect cycles
        self.resolving.push(type_name.to_string());
        let result = self.schema_to_nix_type(&def);
        self.resolving.pop();

        result
    }

    /// Convert a primitive type name to NixType
    fn primitive_to_nix_type(&self, type_name: &str) -> Result<NixType, GeneratorError> {
        match type_name {
            "string" => Ok(NixType::Str),
            "integer" => Ok(NixType::Int),
            "boolean" => Ok(NixType::Bool),
            "number" => Ok(NixType::Float),
            "null" => Ok(NixType::NullOr(Box::new(NixType::Anything))),
            "array" => Ok(NixType::ListOf(Box::new(NixType::Anything))),
            "object" => Ok(NixType::AttrsOf(Box::new(NixType::Anything))),
            _ => Err(GeneratorError::UnsupportedConstruct(format!(
                "unknown primitive type: {}",
                type_name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_primitive_types() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"},
                "enabled": {"type": "boolean"},
                "ratio": {"type": "number"}
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };

        let output = generator.generate(&schema, &options).unwrap();
        assert!(output.contains("lib.types.str"));
        assert!(output.contains("lib.types.int"));
        assert!(output.contains("lib.types.bool"));
        assert!(output.contains("lib.types.number"));
    }

    #[test]
    fn test_nullable_anyof() {
        let schema = json!({
            "type": "object",
            "properties": {
                "color": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "null"}
                    ]
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["color"])
            .unwrap();
        assert!(matches!(nix_type, NixType::NullOr(_)));
    }

    #[test]
    fn test_enum() {
        let schema = json!({
            "type": "object",
            "properties": {
                "direction": {
                    "enum": ["Left", "Right", "Up", "Down"]
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["direction"])
            .unwrap();
        assert!(matches!(nix_type, NixType::Enum(_)));
    }

    #[test]
    fn test_array() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["tags"])
            .unwrap();
        assert!(matches!(nix_type, NixType::ListOf(_)));
    }

    #[test]
    fn test_ref_resolution() {
        let schema = json!({
            "$defs": {
                "Color": {
                    "enum": ["Red", "Green", "Blue"]
                }
            },
            "type": "object",
            "properties": {
                "bg": {"$ref": "#/$defs/Color"}
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["bg"])
            .unwrap();
        assert!(matches!(nix_type, NixType::Enum(_)));
    }

    #[test]
    fn test_object_submodule() {
        let schema = json!({
            "type": "object",
            "properties": {
                "window": {
                    "type": "object",
                    "properties": {
                        "width": {"type": "integer"},
                        "height": {"type": "integer"}
                    }
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["window"])
            .unwrap();
        assert!(matches!(nix_type, NixType::Submodule(_)));
    }

    #[test]
    fn test_attrs_of() {
        let schema = json!({
            "type": "object",
            "properties": {
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"}
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["env"])
            .unwrap();
        assert!(matches!(nix_type, NixType::AttrsOf(_)));
    }

    #[test]
    fn test_nullable_array_with_items() {
        // This is the pattern: "type": ["array", "null"], "items": {"type": "string"}
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": ["array", "null"],
                    "items": {"type": "string"}
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["tags"])
            .unwrap();

        // Should be nullOr (listOf str), not nullOr (listOf anything)
        match nix_type {
            NixType::NullOr(inner) => match *inner {
                NixType::ListOf(item_type) => {
                    assert!(
                        matches!(*item_type, NixType::Str),
                        "Expected Str, got {:?}",
                        item_type
                    );
                }
                _ => panic!("Expected ListOf, got {:?}", inner),
            },
            _ => panic!("Expected NullOr, got {:?}", nix_type),
        }
    }

    #[test]
    fn test_nullable_array_with_ref_items() {
        // Pattern: "type": ["array", "null"], "items": {"$ref": "#/$defs/SomeType"}
        let schema = json!({
            "$defs": {
                "Color": {"enum": ["Red", "Green", "Blue"]}
            },
            "type": "object",
            "properties": {
                "colors": {
                    "type": ["array", "null"],
                    "items": {"$ref": "#/$defs/Color"}
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["colors"])
            .unwrap();

        // Should be nullOr (listOf (enum [...])), not nullOr (listOf anything)
        match nix_type {
            NixType::NullOr(inner) => match *inner {
                NixType::ListOf(item_type) => {
                    assert!(
                        matches!(*item_type, NixType::Enum(_)),
                        "Expected Enum, got {:?}",
                        item_type
                    );
                }
                _ => panic!("Expected ListOf, got {:?}", inner),
            },
            _ => panic!("Expected NullOr, got {:?}", nix_type),
        }
    }

    #[test]
    fn test_nullable_object_with_pattern_properties() {
        // Pattern: "type": ["object", "null"], "patternProperties": {...}
        let schema = json!({
            "type": "object",
            "properties": {
                "display_prefs": {
                    "type": ["object", "null"],
                    "patternProperties": {
                        "^\\d+$": {"type": "string"}
                    }
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator
            .schema_to_nix_type(&schema["properties"]["display_prefs"])
            .unwrap();

        // Should be nullOr (attrsOf str), not nullOr anything
        match nix_type {
            NixType::NullOr(inner) => match *inner {
                NixType::AttrsOf(value_type) => {
                    assert!(
                        matches!(*value_type, NixType::Str),
                        "Expected Str, got {:?}",
                        value_type
                    );
                }
                _ => panic!("Expected AttrsOf, got {:?}", inner),
            },
            _ => panic!("Expected NullOr, got {:?}", nix_type),
        }
    }

    #[test]
    fn test_externally_tagged_union() {
        // Externally tagged union pattern like WidgetConfig
        let schema = json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "Komorebi": {
                            "type": "object",
                            "properties": {
                                "enable": {"type": "boolean"}
                            }
                        }
                    },
                    "required": ["Komorebi"]
                },
                {
                    "type": "object",
                    "properties": {
                        "Battery": {
                            "type": "object",
                            "properties": {
                                "enable": {"type": "boolean"}
                            }
                        }
                    },
                    "required": ["Battery"]
                }
            ]
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator.schema_to_nix_type(&schema).unwrap();

        // Should be TaggedUnion, not OneOf
        match nix_type {
            NixType::TaggedUnion(variants) => {
                assert_eq!(variants.len(), 2);
                assert_eq!(variants[0].0, "Komorebi");
                assert_eq!(variants[1].0, "Battery");
            }
            _ => panic!("Expected TaggedUnion, got {:?}", nix_type),
        }
    }

    #[test]
    fn test_mixed_enum_object_union() {
        // Mixed enum + object pattern like DateFormat
        let schema = json!({
            "oneOf": [
                {"type": "string", "const": "DayMonthYear"},
                {"type": "string", "const": "YearMonthDay"},
                {
                    "type": "object",
                    "properties": {
                        "Custom": {"type": "string"}
                    },
                    "required": ["Custom"]
                }
            ]
        });

        let mut generator = Generator::new(&schema);
        let nix_type = generator.schema_to_nix_type(&schema).unwrap();

        // Should be OneOf [Enum, TaggedUnion]
        match nix_type {
            NixType::OneOf(types) => {
                assert_eq!(types.len(), 2);
                assert!(
                    matches!(&types[0], NixType::Enum(v) if v.contains(&"DayMonthYear".to_string()))
                );
                assert!(matches!(&types[1], NixType::TaggedUnion(v) if v[0].0 == "Custom"));
            }
            _ => panic!("Expected OneOf, got {:?}", nix_type),
        }
    }

    #[test]
    fn test_tagged_union_generates_correct_nix() {
        // Test that the generated Nix code has the correct structure
        let schema = json!({
            "type": "object",
            "properties": {
                "widget": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "Battery": {"type": "object", "properties": {"enable": {"type": "boolean"}}}
                            },
                            "required": ["Battery"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "Clock": {"type": "object", "properties": {"format": {"type": "string"}}}
                            },
                            "required": ["Clock"]
                        }
                    ]
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };

        let output = generator.generate(&schema, &options).unwrap();

        // Should contain submodule with optional fields, not oneOf
        assert!(
            output.contains("lib.types.submodule"),
            "Should use submodule"
        );
        assert!(
            output.contains("Battery = lib.mkOption"),
            "Should have Battery option"
        );
        assert!(
            output.contains("Clock = lib.mkOption"),
            "Should have Clock option"
        );
        assert!(output.contains("lib.types.nullOr"), "Should wrap in nullOr");
        assert!(
            output.contains("default = null"),
            "Should have default = null"
        );
        // Should NOT contain oneOf for this pattern
        assert!(
            !output.contains("lib.types.oneOf"),
            "Should NOT use oneOf for tagged unions"
        );
    }

    #[test]
    fn test_recursive_type_deduplication() {
        // Test that nested $refs use TypeRef instead of inlining
        let schema = json!({
            "$defs": {
                "Base16Value": {
                    "enum": ["Base00", "Base01", "Base02"]
                },
                "ThemeOptions": {
                    "type": "object",
                    "properties": {
                        "accent": { "$ref": "#/$defs/Base16Value" },
                        "border": { "$ref": "#/$defs/Base16Value" }
                    }
                }
            },
            "type": "object",
            "properties": {
                "theme1": { "$ref": "#/$defs/ThemeOptions" },
                "theme2": { "$ref": "#/$defs/ThemeOptions" }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };
        let output = generator.generate(&schema, &options).unwrap();

        // base16Value should appear in let bindings
        assert!(
            output.contains("base16Value = (lib.types.enum"),
            "Should have base16Value in let bindings"
        );

        // themeOptions should reference base16Value, not inline the enum
        // The themeOptions submodule should use "type = base16Value;" not inline the enum
        assert!(
            output.contains("type = base16Value;"),
            "ThemeOptions should reference base16Value"
        );

        // The enum should appear exactly once (in the let binding definition)
        let enum_count = output.matches("\"Base00\"").count();
        assert_eq!(
            enum_count, 1,
            "Base16Value enum should appear exactly once, found {} times",
            enum_count
        );
    }

    #[test]
    fn test_topological_sort_handles_cycles() {
        // Test that recursive types don't cause infinite loops
        let schema = json!({
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
            "type": "object",
            "properties": {
                "root": { "$ref": "#/$defs/Node" },
                "root2": { "$ref": "#/$defs/Node" }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };

        // Should not panic or infinite loop
        let output = generator.generate(&schema, &options).unwrap();

        // Cycle should be broken with Anything
        assert!(
            output.contains("lib.types.anything"),
            "Recursive type cycle should be broken with anything"
        );
    }

    #[test]
    fn test_dependency_order() {
        // Leaf types should be generated before composite types
        let schema = json!({
            "$defs": {
                "Color": { "enum": ["Red", "Green", "Blue"] },
                "Style": {
                    "type": "object",
                    "properties": {
                        "fg": { "$ref": "#/$defs/Color" },
                        "bg": { "$ref": "#/$defs/Color" }
                    }
                },
                "Theme": {
                    "type": "object",
                    "properties": {
                        "primary": { "$ref": "#/$defs/Style" },
                        "secondary": { "$ref": "#/$defs/Style" }
                    }
                }
            },
            "type": "object",
            "properties": {
                "light": { "$ref": "#/$defs/Theme" },
                "dark": { "$ref": "#/$defs/Theme" }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };
        let output = generator.generate(&schema, &options).unwrap();

        // Check order in let block: color before style before theme
        let color_pos = output.find("color = ").expect("color should be in output");
        let style_pos = output.find("style = ").expect("style should be in output");
        let theme_pos = output.find("theme = ").expect("theme should be in output");

        assert!(
            color_pos < style_pos,
            "color should be defined before style"
        );
        assert!(
            style_pos < theme_pos,
            "style should be defined before theme"
        );

        // Style should reference color, not inline the enum
        // Find the style definition and check it uses "type = color;"
        let style_section = &output[style_pos..theme_pos];
        assert!(
            style_section.contains("type = color;"),
            "style should reference color type"
        );

        // Theme should reference style with "type = style;"
        let theme_section = &output[theme_pos..];
        assert!(
            theme_section.contains("type = style;"),
            "theme should reference style type"
        );
    }

    #[test]
    fn test_internally_tagged_union_deduplicates_types() {
        // Refs inside internally-tagged unions SHOULD create let bindings
        // when used multiple times, to avoid repetition in output
        let schema = json!({
            "$defs": {
                "Config": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "integer" }
                    }
                },
                "Mode": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "const": "Simple" }
                            },
                            "required": ["kind"]
                        },
                        {
                            "type": "object",
                            "allOf": [
                                { "$ref": "#/$defs/Config" }
                            ],
                            "properties": {
                                "kind": { "type": "string", "const": "Advanced" }
                            },
                            "required": ["kind"]
                        },
                        {
                            "type": "object",
                            "allOf": [
                                { "$ref": "#/$defs/Config" }
                            ],
                            "properties": {
                                "kind": { "type": "string", "const": "Expert" }
                            },
                            "required": ["kind"]
                        }
                    ]
                }
            },
            "type": "object",
            "properties": {
                "mode": { "$ref": "#/$defs/Mode" }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };
        let output = generator.generate(&schema, &options).unwrap();

        // Config is used twice (in Advanced and Expert variants), so it SHOULD get a let binding
        assert!(
            output.contains("config = "),
            "Config should be in let bindings since it's used twice in internally-tagged union"
        );
    }

    #[test]
    fn test_internally_tagged_union_with_shared_ref_types() {
        // Simulates KomorebiTheme pattern: internally-tagged union where each variant
        // has multiple fields referencing the same enum type
        let schema = json!({
            "$defs": {
                "CatppuccinValue": {
                    "type": "string",
                    "enum": ["Rosewater", "Flamingo", "Pink", "Mauve", "Red", "Maroon"]
                },
                "Base16Value": {
                    "type": "string",
                    "enum": ["Base00", "Base01", "Base02", "Base03", "Base04", "Base05"]
                },
                "Theme": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "palette": { "type": "string", "const": "Catppuccin" },
                                "border1": {
                                    "anyOf": [
                                        { "$ref": "#/$defs/CatppuccinValue" },
                                        { "type": "null" }
                                    ]
                                },
                                "border2": {
                                    "anyOf": [
                                        { "$ref": "#/$defs/CatppuccinValue" },
                                        { "type": "null" }
                                    ]
                                },
                                "border3": {
                                    "anyOf": [
                                        { "$ref": "#/$defs/CatppuccinValue" },
                                        { "type": "null" }
                                    ]
                                }
                            },
                            "required": ["palette"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "palette": { "type": "string", "const": "Base16" },
                                "border1": {
                                    "anyOf": [
                                        { "$ref": "#/$defs/Base16Value" },
                                        { "type": "null" }
                                    ]
                                },
                                "border2": {
                                    "anyOf": [
                                        { "$ref": "#/$defs/Base16Value" },
                                        { "type": "null" }
                                    ]
                                },
                                "border3": {
                                    "anyOf": [
                                        { "$ref": "#/$defs/Base16Value" },
                                        { "type": "null" }
                                    ]
                                }
                            },
                            "required": ["palette"]
                        }
                    ]
                }
            },
            "type": "object",
            "properties": {
                "theme": { "$ref": "#/$defs/Theme" }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };
        let output = generator.generate(&schema, &options).unwrap();

        // Both CatppuccinValue (used 3x) and Base16Value (used 3x) should get let bindings
        assert!(
            output.contains("catppuccinValue = (lib.types.enum"),
            "CatppuccinValue should be in let bindings"
        );
        assert!(
            output.contains("base16Value = (lib.types.enum"),
            "Base16Value should be in let bindings"
        );

        // Each enum should appear exactly once (in the let binding definition)
        let catppuccin_count = output.matches("\"Rosewater\"").count();
        assert_eq!(
            catppuccin_count, 1,
            "CatppuccinValue enum should appear exactly once, found {} times",
            catppuccin_count
        );

        let base16_count = output.matches("\"Base00\"").count();
        assert_eq!(
            base16_count, 1,
            "Base16Value enum should appear exactly once, found {} times",
            base16_count
        );
    }

    #[test]
    fn test_internally_tagged_union_single_use_ref_not_deduplicated() {
        // Refs used only once should NOT get let bindings (even inside unions)
        let schema = json!({
            "$defs": {
                "SingleUseType": {
                    "type": "string",
                    "enum": ["A", "B", "C"]
                },
                "Mode": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "const": "Simple" },
                                "value": { "$ref": "#/$defs/SingleUseType" }
                            },
                            "required": ["kind"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "const": "Complex" },
                                "other": { "type": "integer" }
                            },
                            "required": ["kind"]
                        }
                    ]
                }
            },
            "type": "object",
            "properties": {
                "mode": { "$ref": "#/$defs/Mode" }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };
        let output = generator.generate(&schema, &options).unwrap();

        // SingleUseType is used only once, so should NOT get a let binding
        assert!(
            !output.contains("singleUseType = "),
            "SingleUseType should NOT be in let bindings - it's only used once"
        );

        // But it should still be inlined correctly
        assert!(
            output.contains("\"A\"") && output.contains("\"B\"") && output.contains("\"C\""),
            "SingleUseType enum values should be inlined"
        );
    }

    #[test]
    fn test_pattern_properties_ref_counting() {
        // Simulates the layout/layout_rules pattern from komorebi schema
        // where layout_rules uses patternProperties to reference Layout
        let schema = json!({
            "$defs": {
                "Layout": {
                    "enum": ["BSP", "Columns", "Rows"]
                }
            },
            "type": "object",
            "properties": {
                "layout": {
                    "anyOf": [
                        { "$ref": "#/$defs/Layout" },
                        { "type": "null" }
                    ]
                },
                "layout_rules": {
                    "type": ["object", "null"],
                    "additionalProperties": false,
                    "patternProperties": {
                        "^\\d+$": { "$ref": "#/$defs/Layout" }
                    }
                }
            }
        });

        let mut generator = Generator::new(&schema);
        let options = GeneratorOptions {
            root_name: "test".to_string(),
            root_description: "Test config".to_string(),
        };
        let output = generator.generate(&schema, &options).unwrap();

        // Layout should be in let bindings (used twice: layout + layout_rules)
        assert!(
            output.contains("layout = (lib.types.enum"),
            "Layout should be in let bindings"
        );

        // The enum should appear exactly once (in the let binding)
        let enum_count = output.matches("\"BSP\"").count();
        assert_eq!(
            enum_count, 1,
            "Layout enum should appear exactly once, found {} times",
            enum_count
        );
    }
}
