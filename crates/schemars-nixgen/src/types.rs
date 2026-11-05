/// Represents a Nix type expression
#[derive(Debug, Clone, PartialEq)]
pub enum NixType {
    /// lib.types.str
    Str,
    /// lib.types.int
    Int,
    /// lib.types.bool
    Bool,
    /// lib.types.float (actually lib.types.number in Nix)
    Float,
    /// lib.types.path
    Path,
    /// lib.types.nullOr <inner>
    NullOr(Box<NixType>),
    /// lib.types.listOf <inner>
    ListOf(Box<NixType>),
    /// lib.types.attrsOf <inner>
    AttrsOf(Box<NixType>),
    /// lib.types.enum [ "A" "B" ... ]
    Enum(Vec<String>),
    /// lib.types.oneOf [ ... ]
    OneOf(Vec<NixType>),
    /// lib.types.submodule { options = { ... }; }
    Submodule(Vec<NixOption>),
    /// Tagged union: submodule where each variant is an optional field
    /// Generated from externally tagged unions like WidgetConfig
    /// Each tuple is (tag_name, inner_type)
    TaggedUnion(Vec<(String, NixType)>),
    /// Reference to a named type defined in a let binding
    TypeRef(String),
    /// lib.types.anything - fallback for complex types
    Anything,
    /// Raw Nix expression for edge cases
    Raw(String),
}

/// Represents a Nix option definition
#[derive(Debug, Clone, PartialEq)]
pub struct NixOption {
    pub name: String,
    pub nix_type: NixType,
    pub description: Option<String>,
    pub default: Option<String>,
    pub example: Option<String>,
}

impl NixType {
    /// Convert this type to a Nix expression string
    pub fn to_nix_expr(&self) -> String {
        match self {
            NixType::Str => "lib.types.str".to_string(),
            NixType::Int => "lib.types.int".to_string(),
            NixType::Bool => "lib.types.bool".to_string(),
            NixType::Float => "lib.types.number".to_string(),
            NixType::Path => "lib.types.path".to_string(),
            NixType::NullOr(inner) => {
                format!("(lib.types.nullOr {})", inner.to_nix_expr())
            }
            NixType::ListOf(inner) => {
                format!("(lib.types.listOf {})", inner.to_nix_expr())
            }
            NixType::AttrsOf(inner) => {
                format!("(lib.types.attrsOf {})", inner.to_nix_expr())
            }
            NixType::Enum(variants) => {
                let variants_str: Vec<String> = variants
                    .iter()
                    .map(|v| format!("\"{}\"", escape_nix_string(v)))
                    .collect();
                format!("(lib.types.enum [ {} ])", variants_str.join(" "))
            }
            NixType::OneOf(types) => {
                let types_str: Vec<String> = types.iter().map(|t| t.to_nix_expr()).collect();
                format!("(lib.types.oneOf [ {} ])", types_str.join(" "))
            }
            NixType::Submodule(options) => {
                if options.is_empty() {
                    "(lib.types.attrsOf lib.types.anything)".to_string()
                } else {
                    let opts: Vec<String> =
                        options.iter().map(|opt| opt.to_nix_option_def(2)).collect();
                    format!(
                        "(lib.types.submodule {{\n      options = {{\n{}\n      }};\n    }})",
                        opts.join("\n")
                    )
                }
            }
            NixType::TaggedUnion(variants) => {
                // Generate submodule with each variant as nullOr option with default = null
                // This transforms externally-tagged unions into a single submodule where
                // users specify exactly one variant (the others remain null)
                if variants.is_empty() {
                    "(lib.types.attrsOf lib.types.anything)".to_string()
                } else {
                    let opts: Vec<String> = variants
                        .iter()
                        .map(|(tag, inner_type)| {
                            // Each variant becomes: Tag = lib.mkOption { type = lib.types.nullOr <inner>; default = null; };
                            let inner_expr = inner_type.to_nix_expr();
                            let tag_escaped = escape_nix_attr_name(tag);
                            format!(
                                "        {} = lib.mkOption {{ type = (lib.types.nullOr {}); default = null; }};",
                                tag_escaped,
                                inner_expr
                            )
                        })
                        .collect();
                    format!(
                        "(lib.types.submodule {{\n      options = {{\n{}\n      }};\n    }})",
                        opts.join("\n")
                    )
                }
            }
            NixType::TypeRef(name) => name.clone(),
            NixType::Anything => "lib.types.anything".to_string(),
            NixType::Raw(expr) => expr.clone(),
        }
    }

    /// Check if this type is nullable (NullOr)
    pub fn is_nullable(&self) -> bool {
        matches!(self, NixType::NullOr(_))
    }

    /// Wrap this type in NullOr if not already nullable
    pub fn make_nullable(self) -> NixType {
        if self.is_nullable() {
            self
        } else {
            NixType::NullOr(Box::new(self))
        }
    }
}

impl NixOption {
    /// Create a new NixOption
    pub fn new(name: impl Into<String>, nix_type: NixType) -> Self {
        Self {
            name: name.into(),
            nix_type,
            description: None,
            default: None,
            example: None,
        }
    }

    /// Set the description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the default value
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// Generate the Nix option definition code
    pub fn to_nix_option_def(&self, indent_level: usize) -> String {
        let indent = "  ".repeat(indent_level);
        let inner_indent = "  ".repeat(indent_level + 1);

        let mut parts = vec![format!(
            "{}type = {};",
            inner_indent,
            self.nix_type.to_nix_expr()
        )];

        // Add default if present, otherwise use null for nullable types
        if let Some(ref default) = self.default {
            parts.push(format!("{}default = {};", inner_indent, default));
        } else if self.nix_type.is_nullable() {
            parts.push(format!("{}default = null;", inner_indent));
        }

        // Add description if present
        if let Some(ref desc) = self.description {
            parts.push(format!(
                "{}description = \"{}\";",
                inner_indent,
                escape_nix_string(desc)
            ));
        }

        format!(
            "{}{} = lib.mkOption {{\n{}\n{}}};",
            indent,
            escape_nix_attr_name(&self.name),
            parts.join("\n"),
            indent
        )
    }
}

/// Escape a string for use in Nix string literals
pub fn escape_nix_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace("${", "\\${")
}

/// Escape an attribute name if it contains special characters
pub fn escape_nix_attr_name(name: &str) -> String {
    // Nix attribute names that contain hyphens or start with numbers need quoting
    if name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && name.chars().next().is_some_and(|c| !c.is_ascii_digit())
    {
        name.to_string()
    } else {
        format!("\"{}\"", escape_nix_string(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_types() {
        assert_eq!(NixType::Str.to_nix_expr(), "lib.types.str");
        assert_eq!(NixType::Int.to_nix_expr(), "lib.types.int");
        assert_eq!(NixType::Bool.to_nix_expr(), "lib.types.bool");
        assert_eq!(NixType::Float.to_nix_expr(), "lib.types.number");
    }

    #[test]
    fn test_nullable_type() {
        let nullable_str = NixType::NullOr(Box::new(NixType::Str));
        assert_eq!(
            nullable_str.to_nix_expr(),
            "(lib.types.nullOr lib.types.str)"
        );
    }

    #[test]
    fn test_list_type() {
        let list_of_int = NixType::ListOf(Box::new(NixType::Int));
        assert_eq!(
            list_of_int.to_nix_expr(),
            "(lib.types.listOf lib.types.int)"
        );
    }

    #[test]
    fn test_attrs_type() {
        let attrs_of_str = NixType::AttrsOf(Box::new(NixType::Str));
        assert_eq!(
            attrs_of_str.to_nix_expr(),
            "(lib.types.attrsOf lib.types.str)"
        );
    }

    #[test]
    fn test_enum_type() {
        let enum_type = NixType::Enum(vec!["Left".to_string(), "Right".to_string()]);
        assert_eq!(
            enum_type.to_nix_expr(),
            "(lib.types.enum [ \"Left\" \"Right\" ])"
        );
    }

    #[test]
    fn test_escape_nix_string() {
        assert_eq!(escape_nix_string("hello"), "hello");
        assert_eq!(escape_nix_string("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_nix_string("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_nix_string("${var}"), "\\${var}");
    }

    #[test]
    fn test_escape_attr_name() {
        assert_eq!(escape_nix_attr_name("foo"), "foo");
        assert_eq!(escape_nix_attr_name("foo_bar"), "foo_bar");
        assert_eq!(escape_nix_attr_name("foo-bar"), "\"foo-bar\"");
        assert_eq!(escape_nix_attr_name("123start"), "\"123start\"");
    }

    #[test]
    fn test_make_nullable() {
        let str_type = NixType::Str;
        let nullable = str_type.make_nullable();
        assert!(nullable.is_nullable());
        assert_eq!(nullable.to_nix_expr(), "(lib.types.nullOr lib.types.str)");

        // Already nullable should not double-wrap
        let already_nullable = NixType::NullOr(Box::new(NixType::Int));
        let still_nullable = already_nullable.clone().make_nullable();
        assert_eq!(still_nullable, already_nullable);
    }

    #[test]
    fn test_tagged_union_type() {
        let tagged = NixType::TaggedUnion(vec![
            ("Komorebi".to_string(), NixType::Str),
            ("Battery".to_string(), NixType::Int),
        ]);
        let expr = tagged.to_nix_expr();

        // Should produce a submodule with optional fields
        assert!(expr.contains("lib.types.submodule"));
        assert!(expr.contains("Komorebi = lib.mkOption"));
        assert!(expr.contains("Battery = lib.mkOption"));
        assert!(expr.contains("lib.types.nullOr lib.types.str"));
        assert!(expr.contains("lib.types.nullOr lib.types.int"));
        assert!(expr.contains("default = null"));
    }

    #[test]
    fn test_tagged_union_empty() {
        let tagged = NixType::TaggedUnion(vec![]);
        assert_eq!(
            tagged.to_nix_expr(),
            "(lib.types.attrsOf lib.types.anything)"
        );
    }

    #[test]
    fn test_tagged_union_with_submodule() {
        // Test that tagged union can contain submodule types
        let inner_submodule = NixType::Submodule(vec![NixOption::new("enable", NixType::Bool)]);
        let tagged = NixType::TaggedUnion(vec![("Widget".to_string(), inner_submodule)]);
        let expr = tagged.to_nix_expr();

        assert!(expr.contains("Widget = lib.mkOption"));
        assert!(expr.contains("lib.types.nullOr"));
        assert!(expr.contains("lib.types.submodule"));
    }
}
