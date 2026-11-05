//! Nix code formatting utilities

/// Generate the Nix module header
pub fn module_header() -> &'static str {
    "{ lib, ... }:"
}

/// Generate the opening of an options block
pub fn options_block_open(option_name: &str) -> String {
    format!(
        r#"{{
  options.{} = lib.mkOption {{
    type = lib.types.submodule {{
      options = {{"#,
        option_name
    )
}

/// Generate the closing of an options block
pub fn options_block_close(description: &str) -> String {
    format!(
        r#"      }};
    }};
    default = {{}};
    description = "{}";
  }};
}}"#,
        crate::types::escape_nix_string(description)
    )
}

/// Indent a block of text
pub fn indent(text: &str, level: usize) -> String {
    let prefix = "  ".repeat(level);
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert a JSON value to a Nix literal
pub fn json_to_nix_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                format!("{:.6}", f)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            } else {
                n.to_string()
            }
        }
        serde_json::Value::String(s) => {
            format!("\"{}\"", crate::types::escape_nix_string(s))
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(json_to_nix_literal).collect();
            format!("[ {} ]", items.join(" "))
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                let attrs: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{} = {};",
                            crate::types::escape_nix_attr_name(k),
                            json_to_nix_literal(v)
                        )
                    })
                    .collect();
                format!("{{ {} }}", attrs.join(" "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_to_nix_literal_primitives() {
        assert_eq!(json_to_nix_literal(&json!(null)), "null");
        assert_eq!(json_to_nix_literal(&json!(true)), "true");
        assert_eq!(json_to_nix_literal(&json!(false)), "false");
        assert_eq!(json_to_nix_literal(&json!(42)), "42");
        assert_eq!(json_to_nix_literal(&json!(2.5)), "2.5");
        assert_eq!(json_to_nix_literal(&json!("hello")), "\"hello\"");
    }

    #[test]
    fn test_json_to_nix_literal_array() {
        assert_eq!(json_to_nix_literal(&json!([1, 2, 3])), "[ 1 2 3 ]");
        assert_eq!(json_to_nix_literal(&json!(["a", "b"])), "[ \"a\" \"b\" ]");
    }

    #[test]
    fn test_json_to_nix_literal_object() {
        assert_eq!(json_to_nix_literal(&json!({})), "{}");
        // Note: Object ordering may vary, so we just check it produces valid output
        let result = json_to_nix_literal(&json!({"x": 1}));
        assert!(result.contains("x = 1"));
    }

    #[test]
    fn test_indent() {
        let text = "line1\nline2\nline3";
        let indented = indent(text, 2);
        assert_eq!(indented, "    line1\n    line2\n    line3");
    }

    #[test]
    fn test_module_header() {
        assert_eq!(module_header(), "{ lib, ... }:");
    }
}
