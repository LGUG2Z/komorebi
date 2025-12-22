#[path = "src/identifiers.rs"]
mod identifiers;

use identifiers::CSS;
use identifiers::TYPE_CLASSES;
use lightningcss::rules::CssRule;
use lightningcss::selector::Component;
use lightningcss::stylesheet::ParserOptions;
use lightningcss::stylesheet::StyleSheet;
use std::collections::HashSet;
use std::fs;
use struct_iterable::Iterable;

fn main() {
    println!("cargo:rerun-if-changed=src/templates/style.css");
    println!("cargo:rerun-if-changed=src/identifiers.rs");

    let css_content =
        fs::read_to_string("src/templates/style.css").expect("Failed to read style.css");

    // Parse CSS and extract class names
    let css_classes = extract_css_classes(&css_content);

    // Collect all defined classes using struct_iterable's automatic iteration
    let mut defined_classes: HashSet<&str> = HashSet::new();

    // Iterate over CssClasses fields - automatically includes ALL fields
    for (_field_name, field_value) in CSS.iter() {
        if let Some(class) = field_value.downcast_ref::<&str>() {
            defined_classes.insert(*class);
        }
    }

    // Iterate over TypeClasses fields - automatically includes ALL fields
    for (_field_name, field_value) in TYPE_CLASSES.iter() {
        if let Some(class) = field_value.downcast_ref::<&str>() {
            defined_classes.insert(*class);
        }
    }

    // Dynamic classes to skip (generated at runtime)
    let dynamic_prefixes = ["depth-"];

    // Check for CSS classes not defined in identifiers.rs
    let undefined: Vec<&String> = css_classes
        .iter()
        .filter(|c| !defined_classes.contains(c.as_str()))
        .filter(|c| !dynamic_prefixes.iter().any(|p| c.starts_with(p)))
        .collect();

    if !undefined.is_empty() {
        let undefined_list: Vec<&str> = undefined.iter().map(|s| s.as_str()).collect();
        panic!(
            "CSS classes in style.css not defined in identifiers.rs:\n  {}",
            undefined_list.join("\n  ")
        );
    }

    // Warn about defined classes not used in CSS
    for class in &defined_classes {
        if !css_classes.contains(*class) {
            println!("cargo:warning=Class '{}' defined but not styled", class);
        }
    }
}

fn extract_css_classes(css: &str) -> HashSet<String> {
    let mut classes = HashSet::new();

    let stylesheet = StyleSheet::parse(css, ParserOptions::default()).expect("Failed to parse CSS");

    fn collect_from_rules(rules: &[CssRule], classes: &mut HashSet<String>) {
        for rule in rules {
            match rule {
                CssRule::Style(style_rule) => {
                    for selector in style_rule.selectors.0.iter() {
                        for component in selector.iter_raw_match_order() {
                            if let Component::Class(class_name) = component {
                                classes.insert(class_name.0.to_string());
                            }
                        }
                    }
                    collect_from_rules(&style_rule.rules.0, classes);
                }
                CssRule::Media(media_rule) => {
                    collect_from_rules(&media_rule.rules.0, classes);
                }
                _ => {}
            }
        }
    }

    collect_from_rules(&stylesheet.rules.0, &mut classes);
    classes
}
