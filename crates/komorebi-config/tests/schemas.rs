//! The oracle of the crate: the schemas that are checked in next to the three daemons.
//!
//! `schema.json` and `schema.asc.json` of the Windows tree (the repository root), of
//! `komorebi-for-kde` and of `komorebi-for-mac` are what users, the docs and the nix options
//! are generated from, so they must not change by accident while the data types move into this
//! crate. The files are read by their path relative to the repository.
//!
//! What is checked at this stage:
//! - every checked-in file is in the canonical form the generator prints (pretty JSON, one
//!   trailing newline), so a regenerated schema can be compared with `==` on the text;
//! - the files keep the keys they have today (53/37/33 properties, 60/52/47 definitions and
//!   the six definitions of the applications schema): a step that drops or adds a key of
//!   `komorebi.json` for one platform fails here.

use std::path::PathBuf;

use serde_json::Value;

struct Tree {
    name: &'static str,
    dir: &'static str,
    properties: usize,
    defs: usize,
}

const TREES: [Tree; 3] = [
    Tree {
        name: "windows",
        dir: "",
        properties: 53,
        defs: 60,
    },
    Tree {
        name: "kde",
        dir: "komorebi-for-kde",
        properties: 37,
        defs: 52,
    },
    Tree {
        name: "macos",
        dir: "komorebi-for-mac",
        properties: 33,
        defs: 47,
    },
];

fn checked_in(tree: &Tree, file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(tree.dir)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_checked_in_schemas_are_in_the_form_the_generator_prints() {
    for tree in &TREES {
        for file in ["schema.json", "schema.asc.json"] {
            let text = checked_in(tree, file);
            let value: Value = serde_json::from_str(&text).expect("a schema is JSON");
            let mut again = serde_json::to_string_pretty(&value).expect("JSON");
            again.push('\n');
            assert!(
                text == again,
                "{} {file} is not in canonical form",
                tree.name
            );
        }
    }
}

#[test]
fn the_checked_in_schemas_have_the_keys_of_each_platform() {
    for tree in &TREES {
        let schema: Value = serde_json::from_str(&checked_in(tree, "schema.json")).unwrap();
        assert_eq!(schema["title"], "StaticConfig", "{}", tree.name);
        assert_eq!(
            schema["properties"].as_object().unwrap().len(),
            tree.properties,
            "{} properties",
            tree.name
        );
        assert_eq!(
            schema["$defs"].as_object().unwrap().len(),
            tree.defs,
            "{} definitions",
            tree.name
        );

        let asc: Value = serde_json::from_str(&checked_in(tree, "schema.asc.json")).unwrap();
        let defs: Vec<&str> = asc["$defs"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            defs,
            [
                "ApplicationIdentifier",
                "AscApplicationRules",
                "AscApplicationRulesOrSchema",
                "IdWithIdentifier",
                "MatchingRule",
                "MatchingStrategy"
            ],
            "{} applications schema",
            tree.name
        );
    }
}

/// The definitions schemars generates for `T` and for everything `T` refers to, by name.
fn generated<T: schemars::JsonSchema>() -> serde_json::Map<String, Value> {
    let mut generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let _ = generator.subschema_for::<T>();
    let defs = generator.take_definitions(true);
    match serde_json::to_value(&defs).expect("definitions are JSON") {
        Value::Object(map) => map,
        other => panic!("definitions are an object, not {other}"),
    }
}

/// The definitions `owned` by this crate (the others come with `T` but live in the layouts,
/// themes or protocol crates and have their own checks) must be in each of the `trees`'
/// `schema.json` and equal to what is generated for `T`.
fn assert_same_definitions<T: schemars::JsonSchema>(trees: &[&Tree], owned: &[&str]) {
    let generated = generated::<T>();
    for tree in trees {
        let schema: Value = serde_json::from_str(&checked_in(tree, "schema.json")).unwrap();
        let defs = schema["$defs"].as_object().unwrap();
        for name in owned {
            assert!(
                defs.contains_key(*name),
                "{} schema.json has no {name}",
                tree.name
            );
            assert!(generated.contains_key(*name), "{name} was not generated");
        }
        for name in owned {
            let def = &generated[*name];
            if let Some(checked) = defs.get(*name) {
                assert!(
                    checked == def,
                    "{} schema.json: the definition of {name} differs from the generated one:\n{}\n---\n{}",
                    tree.name,
                    serde_json::to_string_pretty(checked).unwrap(),
                    serde_json::to_string_pretty(def).unwrap(),
                );
            }
        }
    }
}

fn all() -> Vec<&'static Tree> {
    TREES.iter().collect()
}

#[test]
fn the_matching_types_are_the_definitions_of_every_tree() {
    use komorebi_config::matching::MatchingRule;
    use komorebi_config::matching::WorkspaceMatchingRule;

    assert_same_definitions::<MatchingRule>(
        &all(),
        &["MatchingRule", "IdWithIdentifier", "MatchingStrategy"],
    );
    // The workspace rules are an input of the daemons only: no schema carries them.
    let _ = generated::<WorkspaceMatchingRule>();
}

#[test]
fn the_behaviour_enums_are_the_definitions_of_the_trees_that_have_them() {
    use komorebi_config::behaviour::CrossBoundaryBehaviour;
    use komorebi_config::behaviour::FloatingLayerBehaviour;
    use komorebi_config::behaviour::Placement;
    use komorebi_config::behaviour::WindowContainerBehaviour;
    use komorebi_config::behaviour::WindowHandlingBehaviour;
    use komorebi_config::behaviour::WindowHidingPosition;
    use komorebi_config::behaviour::ZOrder;

    let [windows, kde, macos] = &TREES;
    assert_same_definitions::<WindowContainerBehaviour>(&all(), &["WindowContainerBehaviour"]);
    assert_same_definitions::<FloatingLayerBehaviour>(&all(), &["FloatingLayerBehaviour"]);
    assert_same_definitions::<Placement>(&all(), &["Placement"]);
    assert_same_definitions::<CrossBoundaryBehaviour>(&all(), &["CrossBoundaryBehaviour"]);
    assert_same_definitions::<WindowHidingPosition>(&[kde, macos], &["WindowHidingPosition"]);
    assert_same_definitions::<WindowHandlingBehaviour>(&[windows], &["WindowHandlingBehaviour"]);
    assert_same_definitions::<ZOrder>(&[windows], &["ZOrder"]);
}
