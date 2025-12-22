//! Single source of truth for all CSS classes and HTML IDs
//!
//! This module centralizes all identifier strings used in the generated HTML output.
//! By defining them here, we ensure consistency between Rust code, HTML templates,
//! CSS styles, and JavaScript.

use serde::Serialize;
use struct_iterable::Iterable;

/// Static HTML element IDs used for JavaScript interaction
#[allow(dead_code)] // Used by main crate, not build script
#[derive(Debug, Clone, Serialize)]
pub struct HtmlIds {
    pub search: &'static str,
    pub search_results: &'static str,
    pub sidebar: &'static str,
    pub sidebar_toggle: &'static str,
    pub root_schema: &'static str,
    pub definitions: &'static str,
    pub modal_overlay: &'static str,
}

/// All static HTML IDs
#[allow(dead_code)] // Used by main crate, not build script
pub const IDS: HtmlIds = HtmlIds {
    search: "search",
    search_results: "search-results",
    sidebar: "sidebar",
    sidebar_toggle: "sidebar-toggle",
    root_schema: "root-schema",
    definitions: "definitions",
    modal_overlay: "modal-overlay",
};

/// CSS class names organized by category
#[derive(Debug, Clone, Serialize, Iterable)]
pub struct CssClasses {
    // Layout
    pub header: &'static str,
    pub header_content: &'static str,
    pub layout: &'static str,
    pub main_content: &'static str,

    // Sidebar
    pub sidebar: &'static str,
    pub sidebar_open: &'static str,
    pub sidebar_header: &'static str,
    pub sidebar_toggle: &'static str,
    pub sidebar_section: &'static str,
    pub sidebar_section_title: &'static str,

    // Navigation
    pub nav_list: &'static str,
    pub nav_link: &'static str,
    pub outline_list: &'static str,
    pub outline_item: &'static str,
    pub outline_children: &'static str,

    // Search
    pub search_container: &'static str,
    pub search_results: &'static str,
    pub search_results_active: &'static str,
    pub search_result_item: &'static str,
    pub no_results: &'static str,
    pub result_name: &'static str,
    pub result_path: &'static str,
    pub result_type: &'static str,

    // Schema sections
    pub schema_section: &'static str,
    pub schema_inline: &'static str,
    pub properties_container: &'static str,

    // Property
    pub property: &'static str,
    pub property_header: &'static str,
    pub property_name: &'static str,
    pub pattern: &'static str,

    // Badges
    pub badge: &'static str,
    pub required: &'static str,
    pub deprecated: &'static str,
    pub optional: &'static str,

    // Type badges
    pub type_badge: &'static str,
    pub type_ref: &'static str,

    // Map display
    pub map_key: &'static str,
    pub key_type: &'static str,
    pub map_arrow: &'static str,

    // Nested schema
    pub nested_schema: &'static str,
    pub enum_details: &'static str,
    pub array_items: &'static str,
    pub tuple_items: &'static str,
    pub tuple_item: &'static str,
    pub tuple_index: &'static str,

    // Constraints
    pub constraints: &'static str,
    pub constraint: &'static str,
    pub label: &'static str,

    // Variants
    pub variants: &'static str,
    pub variant: &'static str,
    pub enum_values: &'static str,

    // Values
    pub default_value: &'static str,
    pub const_value: &'static str,

    // Definition
    pub definition: &'static str,
    pub definition_name: &'static str,
    pub highlight: &'static str,

    // Description
    pub description: &'static str,

    // Usages
    pub usages: &'static str,
    pub usage_section: &'static str,
    pub usage_list: &'static str,

    // Modal
    pub modal_overlay: &'static str,
    pub modal: &'static str,
    pub modal_active: &'static str,
    pub modal_header: &'static str,
    pub modal_title: &'static str,
    pub modal_close: &'static str,
    pub modal_content: &'static str,
    pub modal_nav: &'static str,
    pub modal_nav_btn: &'static str,
    pub modal_nav_disabled: &'static str,
}

/// All CSS class names
pub const CSS: CssClasses = CssClasses {
    // Layout
    header: "header",
    header_content: "header-content",
    layout: "layout",
    main_content: "main-content",

    // Sidebar
    sidebar: "sidebar",
    sidebar_open: "open",
    sidebar_header: "sidebar-header",
    sidebar_toggle: "sidebar-toggle",
    sidebar_section: "sidebar-section",
    sidebar_section_title: "sidebar-section-title",

    // Navigation
    nav_list: "nav-list",
    nav_link: "nav-link",
    outline_list: "outline-list",
    outline_item: "outline-item",
    outline_children: "outline-children",

    // Search
    search_container: "search-container",
    search_results: "search-results",
    search_results_active: "active",
    search_result_item: "search-result-item",
    no_results: "no-results",
    result_name: "result-name",
    result_path: "result-path",
    result_type: "result-type",

    // Schema sections
    schema_section: "schema-section",
    schema_inline: "schema-inline",
    properties_container: "properties-container",

    // Property
    property: "property",
    property_header: "property-header",
    property_name: "property-name",
    pattern: "pattern",

    // Badges
    badge: "badge",
    required: "required",
    deprecated: "deprecated",
    optional: "optional",

    // Type badges
    type_badge: "type-badge",
    type_ref: "type-ref",

    // Map display
    map_key: "map-key",
    key_type: "key-type",
    map_arrow: "map-arrow",

    // Nested schema
    nested_schema: "nested-schema",
    enum_details: "enum-details",
    array_items: "array-items",
    tuple_items: "tuple-items",
    tuple_item: "tuple-item",
    tuple_index: "tuple-index",

    // Constraints
    constraints: "constraints",
    constraint: "constraint",
    label: "label",

    // Variants
    variants: "variants",
    variant: "variant",
    enum_values: "enum-values",

    // Values
    default_value: "default-value",
    const_value: "const-value",

    // Definition
    definition: "definition",
    definition_name: "definition-name",
    highlight: "highlight",

    // Description
    description: "description",

    // Usages
    usages: "usages",
    usage_section: "usage-section",
    usage_list: "usage-list",

    // Modal
    modal_overlay: "modal-overlay",
    modal: "modal",
    modal_active: "modal-active",
    modal_header: "modal-header",
    modal_title: "modal-title",
    modal_close: "modal-close",
    modal_content: "modal-content",
    modal_nav: "modal-nav",
    modal_nav_btn: "modal-nav-btn",
    modal_nav_disabled: "disabled",
};

/// Type-specific badge class suffixes (used with type-badge prefix)
#[derive(Debug, Clone, Serialize, Iterable)]
pub struct TypeClasses {
    pub string: &'static str,
    pub number: &'static str,
    pub integer: &'static str,
    pub float: &'static str,
    pub boolean: &'static str,
    pub null: &'static str,
    pub object: &'static str,
    pub array: &'static str,
    pub r#enum: &'static str,
    pub union: &'static str,
    pub r#const: &'static str,
}

/// Type-specific badge classes
pub const TYPE_CLASSES: TypeClasses = TypeClasses {
    string: "type-string",
    number: "type-number",
    integer: "type-integer",
    float: "type-float",
    boolean: "type-boolean",
    null: "type-null",
    object: "type-object",
    array: "type-array",
    r#enum: "type-enum",
    union: "type-union",
    r#const: "type-const",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ids_are_valid_html_ids() {
        // HTML IDs should not contain spaces and should be lowercase
        assert!(!IDS.search.contains(' '));
        assert!(!IDS.search_results.contains(' '));
        assert!(!IDS.sidebar.contains(' '));
        assert!(!IDS.sidebar_toggle.contains(' '));
    }

    #[test]
    fn test_css_classes_are_valid() {
        // CSS classes should not contain spaces (multiple classes are separate fields)
        assert!(!CSS.header.contains(' '));
        assert!(!CSS.property.contains(' '));
        assert!(!CSS.type_badge.contains(' '));
    }

    #[test]
    fn test_type_classes_have_prefix() {
        // All type classes should start with "type-"
        assert!(TYPE_CLASSES.string.starts_with("type-"));
        assert!(TYPE_CLASSES.integer.starts_with("type-"));
        assert!(TYPE_CLASSES.array.starts_with("type-"));
    }

    #[test]
    fn test_modal_nav_classes_are_valid() {
        // Modal navigation CSS classes should not contain spaces
        assert!(!CSS.modal_nav.contains(' '));
        assert!(!CSS.modal_nav_btn.contains(' '));
        assert!(!CSS.modal_nav_disabled.contains(' '));
    }

    #[test]
    fn test_all_css_classes_are_styled() {
        use regex::Regex;
        use std::collections::HashSet;
        use std::fs;
        use struct_iterable::Iterable;

        let css_content =
            fs::read_to_string("src/templates/style.css").expect("Failed to read style.css");

        // Extract all class names from CSS using regex
        let class_regex = Regex::new(r"\.([a-zA-Z_][a-zA-Z0-9_-]*)").unwrap();
        let css_classes: HashSet<&str> = class_regex
            .captures_iter(&css_content)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect();

        // Verify enum-details specifically (was false-positive before fix)
        assert!(
            css_classes.contains("enum-details"),
            "enum-details should be found in CSS (compound selector .enum-details .enum-values)"
        );

        // Dynamic prefixes to skip
        let dynamic_prefixes = ["depth-"];

        // Collect all defined classes and check they're in CSS
        let mut missing: Vec<&str> = Vec::new();

        for (_field_name, field_value) in CSS.iter() {
            if let Some(class) = field_value.downcast_ref::<&str>() {
                if !css_classes.contains(*class)
                    && !dynamic_prefixes.iter().any(|p| class.starts_with(p))
                {
                    missing.push(*class);
                }
            }
        }

        for (_field_name, field_value) in TYPE_CLASSES.iter() {
            if let Some(class) = field_value.downcast_ref::<&str>() {
                if !css_classes.contains(*class) {
                    missing.push(*class);
                }
            }
        }

        assert!(
            missing.is_empty(),
            "CSS classes defined but not styled: {:?}",
            missing
        );
    }
}
