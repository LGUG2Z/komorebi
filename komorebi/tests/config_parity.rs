//! Can one `komorebi.json` be shared across platforms? The fixtures are the file this repository
//! ships (`docs/komorebi.example.json`) and files of komorebi for Windows, which has keys that
//! mean nothing on KDE Plasma. Every fixture must deserialize with [`StaticConfig`], round-trip
//! through its serializer, and lose only the top-level keys that are Windows-only
//! ([`WINDOWS_ONLY`]) or that are read once the feature they configure is there ([`NOT_YET`]).
//! Run with `--nocapture` to see exactly which paths each config has silently dropped.
//!
//! Extra configs (for example your own) can be checked without copying them into the repo:
//!   KOMOREBI_PARITY_EXTRA=~/.config/komorebi/komorebi.json cargo test -p komorebi --test config_parity -- --nocapture

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use komorebi::static_config::StaticConfig;
use serde_json::Value;

/// Top-level fields of upstream (Windows) `StaticConfig` that this one lacks for good, from
/// diffing the two `pub struct StaticConfig` definitions.
const WINDOWS_ONLY: &[&str] = &[
    "bar_configurations",
    "border_implementation",
    "border_overflow_applications",
    "border_style",
    "border_z_order",
    "invisible_borders",
    "layered_applications",
    "minimum_window_height",
    "minimum_window_width",
    "monitor_index_preferences",
    "object_name_change_applications",
    "object_name_change_title_ignore_list",
    "remove_titlebar_applications",
    "slow_application_compensation_time",
    "slow_application_identifiers",
    "stackbar",
    "tray_and_multi_window_applications",
    "window_handling_behaviour",
    "window_hiding_behaviour",
];

/// Top-level fields that `StaticConfig` has when the feature they configure is there, and that are
/// ignored until then. Each comment says which commit of the plan brings the feature, and takes
/// the key off this list.
const NOT_YET: &[&str] = &[
    // application specific configuration (39)
    "app_specific_configuration_path",
    // borders drawn by the compositor (43)
    "border",
    "border_colours",
    "border_offset",
    "border_radius",
    "border_width",
    // animation (45)
    "animation",
    // transparency (47)
    "transparency",
    "transparency_alpha",
    "transparency_ignore_rules",
    // focus follows mouse (48)
    "focus_follows_mouse",
    // themes (49)
    "theme",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixtures() -> Vec<PathBuf> {
    let root = repo_root();
    let mut out = vec![root.join("docs/komorebi.example.json")];
    let dir = root.join("spikes/config-parity/fixtures");
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json") {
                out.push(p);
            }
        }
    }
    if let Ok(extra) = std::env::var("KOMOREBI_PARITY_EXTRA") {
        out.extend(
            extra
                .split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }
    out.sort();
    out
}

/// Every key path present in `input` but absent from `output` (what serde threw away).
fn dropped(input: &Value, output: &Value, path: &str, out: &mut Vec<String>) {
    match (input, output) {
        (Value::Object(i), Value::Object(o)) => {
            for (k, v) in i {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match o.get(k) {
                    Some(ov) => dropped(v, ov, &p, out),
                    None if v.is_null() => {}
                    None => out.push(p),
                }
            }
        }
        (Value::Array(i), Value::Array(o)) => {
            for (n, (iv, ov)) in i.iter().zip(o).enumerate() {
                dropped(iv, ov, &format!("{path}[{n}]"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn fixtures_load_round_trip_and_only_lose_windows_only_keys() {
    let windows_only: BTreeSet<&str> = WINDOWS_ONLY.iter().chain(NOT_YET).copied().collect();
    let files = fixtures();
    assert!(
        files.len() >= 3,
        "expected the repo fixtures, found {files:?}"
    );

    for file in files {
        let name = file
            .strip_prefix(repo_root())
            .unwrap_or(&file)
            .display()
            .to_string();
        let raw = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
        let config =
            StaticConfig::read_raw(&raw).unwrap_or_else(|e| panic!("{name} failed to load: {e}"));

        // round trip through our own serializer
        let again = serde_json::to_string(&config).unwrap();
        let reloaded =
            StaticConfig::read_raw(&again).unwrap_or_else(|e| panic!("{name} round trip: {e}"));
        assert_eq!(config, reloaded, "{name}: round trip changed the config");

        // what did serde silently ignore?
        let input: Value = serde_json::from_str(&raw).unwrap();
        let mut lost = Vec::new();
        dropped(
            &input,
            &serde_json::to_value(&config).unwrap(),
            "",
            &mut lost,
        );
        println!("{name}: loaded; {} paths ignored", lost.len());
        for p in &lost {
            println!("    ignored: {p}");
        }

        let unexpected: Vec<_> = lost
            .iter()
            .filter(|p| !p.contains('.') && !p.contains('['))
            .filter(|p| p.as_str() != "$schema" && !windows_only.contains(p.as_str()))
            .collect();
        assert!(
            unexpected.is_empty(),
            "{name}: dropped top-level keys that are neither Windows-only nor not yet read: {unexpected:?}"
        );
    }
}

/// The top-level keys `StaticConfig` reads, from its schema
fn keys_that_are_read() -> BTreeSet<String> {
    let schema = serde_json::to_value(schemars::schema_for!(StaticConfig)).unwrap();
    schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_lists_name_no_key_that_is_read() {
    let read = keys_that_are_read();

    let stale: Vec<_> = WINDOWS_ONLY
        .iter()
        .chain(NOT_YET)
        .filter(|key| read.contains(**key))
        .collect();

    assert!(
        stale.is_empty(),
        "StaticConfig reads these keys, which are still listed as ignored: {stale:?}"
    );
}

#[test]
fn the_example_in_the_docs_is_the_example_file() {
    let page = std::fs::read_to_string(repo_root().join("docs/example-configurations.md")).unwrap();
    let file = std::fs::read_to_string(repo_root().join("docs/komorebi.example.json")).unwrap();

    let start = page.find("```json\n").expect("the page has the example") + "```json\n".len();
    let end = start + page[start..].find("```").expect("the example ends");

    assert_eq!(page[start..end].trim(), file.trim());
}
