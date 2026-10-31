//! Wire conformance of the protocol crate against the per-platform goldens.
//!
//! The goldens in `tests/golden/<platform>/` were generated from the three daemons'
//! own `SocketMessage` before the type moved into this crate (generator:
//! `tests/common/golden_gen.rs`). `variants.json` holds, per instance, the exact text
//! the socket carries (`wire`) and the re-serialised value (`golden`);
//! `schema-variants.txt` lists the variants of the platform's schema.
//!
//! Stage 2a (SocketMessage has not moved yet) checks:
//! - the goldens are internally consistent (every `wire` text parses to `golden`);
//! - the variant names of each golden equal the platform's column of
//!   `variants-table.tsv`, which `platforms()` is derived from;
//! - the generator itself works, on a stand-in enum.
//!
//! Later stages replace the stand-in with the crate's `SocketMessage` and compare
//! wire text and schema branches per platform (a subset check: the unified enum
//! accepts everything each platform accepted).

#[path = "common/golden_gen.rs"]
#[allow(dead_code)]
mod golden_gen;

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

const PLATFORMS: [(&str, usize); 3] = [("windows", 1), ("kde", 2), ("macos", 3)];

fn golden(platform: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(platform).join("variants.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn schema_variants(platform: &str) -> BTreeSet<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(platform).join("schema-variants.txt");
    std::fs::read_to_string(p).unwrap().lines().map(str::to_owned).collect()
}

fn table() -> Vec<Vec<String>> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/variants-table.tsv");
    std::fs::read_to_string(p).unwrap().lines().skip(1).map(|l| l.split('\t').map(str::to_owned).collect()).collect()
}

#[test]
fn golden_wire_text_matches_golden_value() {
    for (platform, _) in PLATFORMS {
        let g = golden(platform);
        let g = g.as_object().unwrap();
        assert!(!g.is_empty());
        for (name, v) in g {
            if v["deserialised"] != Value::Bool(true) {
                panic!("{platform}: {name} was not accepted by the platform's own enum");
            }
            let wire: Value = serde_json::from_str(v["wire"].as_str().unwrap()).unwrap();
            assert_eq!(wire, v["golden"], "{platform}: {name}");
        }
    }
}

#[test]
fn golden_variants_match_the_support_table() {
    let rows = table();
    assert_eq!(rows.len(), 197);
    for (platform, col) in PLATFORMS {
        let from_table: BTreeSet<String> = rows.iter().filter(|r| r[col] != "-").map(|r| r[0].clone()).collect();
        assert_eq!(from_table, schema_variants(platform), "{platform}");
        // every golden instance is of a variant of the platform (or one of its serde aliases)
        let g = golden(platform);
        for (name, v) in g.as_object().unwrap() {
            let ty = v["golden"]["type"].as_str().unwrap();
            assert!(from_table.contains(ty), "{platform}: {name} serialises as {ty}, which the table does not list");
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", content = "content")]
enum Stand {
    Unit,
    Number(usize),
    Pair(bool, String),
    Maybe(Option<u8>),
}

#[test]
fn generator_runs_on_a_stand_in_enum() {
    let dir = std::env::temp_dir().join(format!("komorebi-protocol-golden-{}", std::process::id()));
    golden_gen::run::<Stand>("stand-in", &dir, &[], &[]);
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("variants.json")).unwrap()).unwrap();
    for k in ["Unit", "Number", "Pair", "Maybe"] {
        assert_eq!(v[k]["deserialised"], Value::Bool(true), "{k}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
